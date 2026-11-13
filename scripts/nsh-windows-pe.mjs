const PE_SIGNATURE = 0x00004550;
const AMD64 = 0x8664;
const PE32_PLUS = 0x20b;
const WINDOWS_GUI = 2;
const IMAGE_EXECUTABLE = 0x0002;
const IMAGE_DLL = 0x2000;
const MAX_IMPORT_NAME_BYTES = 1024;
const MIN_STACK_BYTES = 8n * 1024n * 1024n;
const HIGH_BIT = 0x80000000;
const OFFSET_MASK = 0x7fffffff;
const SECTION_BYTES = 40;
const RESOURCE_HEADER_BYTES = 16;
const RESOURCE_ENTRY_BYTES = 8;
const RESOURCE_TYPES = { icon: 3, groupIcon: 14, manifest: 24 };
const DYNAMIC_CRT = /^(?:msvcp.*|msvcr.*|vcruntime.*|ucrtbase(?:d)?|api-ms-win-crt-.*)\.dll$/i;

function requirePe(condition, message) {
  if (!condition) throw new Error(`Invalid Windows PE: ${message}`);
}

function range(buffer, offset, length) {
  requirePe(
    Number.isSafeInteger(offset) && offset >= 0 && length >= 0 && offset + length <= buffer.length,
    'truncated or out-of-range data',
  );
  return buffer.subarray(offset, offset + length);
}

export function validateWindowsPe(buffer) {
  const dos = range(buffer, 0, 64);
  requirePe(dos.toString('ascii', 0, 2) === 'MZ', 'missing DOS signature');
  const pe = dos.readUInt32LE(60);
  requirePe(range(buffer, pe, 4).readUInt32LE(0) === PE_SIGNATURE, 'missing PE signature');
  const coff = range(buffer, pe + 4, 20);
  requirePe(coff.readUInt16LE(0) === AMD64, 'expected AMD64 desktop executable');
  const characteristics = coff.readUInt16LE(18);
  requirePe(
    characteristics & IMAGE_EXECUTABLE && !(characteristics & IMAGE_DLL),
    'expected executable image',
  );
  const sectionCount = coff.readUInt16LE(2);
  requirePe(sectionCount > 0 && sectionCount <= 96, 'invalid section count');
  const optionalSize = coff.readUInt16LE(16);
  const optional = range(buffer, pe + 24, optionalSize);
  requirePe(optional.length >= 136 && optional.readUInt16LE(0) === PE32_PLUS, 'expected PE32+');
  requirePe(optional.readUInt16LE(68) === WINDOWS_GUI, 'expected Windows GUI subsystem');
  requirePe(optional.readBigUInt64LE(72) >= MIN_STACK_BYTES, 'desktop stack must retain 8 MiB');
  const directoryCount = optional.readUInt32LE(108);
  requirePe(
    directoryCount >= 3 && 112 + directoryCount * 8 <= optional.length,
    'invalid directories',
  );
  const sections = Array.from({ length: sectionCount }, (_, index) => {
    const section = range(buffer, pe + 24 + optionalSize + index * SECTION_BYTES, SECTION_BYTES);
    return {
      rva: section.readUInt32LE(12),
      size: section.readUInt32LE(16),
      offset: section.readUInt32LE(20),
    };
  });
  const map = (rva, length) => {
    const section = sections.find(
      (item) => rva >= item.rva && rva - item.rva + length <= item.size,
    );
    requirePe(section, 'RVA does not map to file data');
    return range(buffer, section.offset + rva - section.rva, length);
  };
  const directory = (index) => ({
    rva: optional.readUInt32LE(112 + index * 8),
    size: optional.readUInt32LE(116 + index * 8),
  });
  const imports = readImports(directory(1), map, 20, 12);
  if (directoryCount > 13) imports.push(...readImports(directory(13), map, 32, 4));
  for (const name of imports) requirePe(!DYNAMIC_CRT.test(name), `dynamic CRT dependency: ${name}`);
  const resources = directory(2);
  requirePe(resources.rva && resources.size, 'missing icon and manifest resources');
  const root = map(resources.rva, resources.size);
  const icons = resourcePayloads(root, RESOURCE_TYPES.icon, map);
  const groups = resourcePayloads(root, RESOURCE_TYPES.groupIcon, map);
  const manifests = resourcePayloads(root, RESOURCE_TYPES.manifest, map);
  validateIconGroups(groups, icons);
  for (const { bytes } of manifests) validateManifest(bytes.toString('utf8'));
  return { imports, iconCount: icons.length, manifestCount: manifests.length };
}

function readImports(directory, map, descriptorBytes, nameOffset) {
  if (!directory.rva && !directory.size) return [];
  requirePe(directory.rva && directory.size >= descriptorBytes, 'invalid import directory');
  const descriptors = map(directory.rva, directory.size);
  const imports = [];
  for (let offset = 0; offset + descriptorBytes <= descriptors.length; offset += descriptorBytes) {
    const descriptor = descriptors.subarray(offset, offset + descriptorBytes);
    if (descriptor.every((byte) => byte === 0)) return imports;
    if (descriptorBytes === 32)
      requirePe(descriptor.readUInt32LE(0) === 1, 'unsupported delay import');
    const nameRva = descriptor.readUInt32LE(nameOffset);
    const name = [];
    for (let index = 0; index < MAX_IMPORT_NAME_BYTES; index += 1) {
      const byte = map(nameRva + index, 1)[0];
      if (byte === 0) break;
      name.push(byte);
    }
    requirePe(name.length > 0 && name.length < MAX_IMPORT_NAME_BYTES, 'invalid import name');
    imports.push(Buffer.from(name).toString('ascii'));
  }
  throw new Error('Invalid Windows PE: unterminated import directory');
}

function entries(root, offset) {
  const header = range(root, offset, RESOURCE_HEADER_BYTES);
  const count = header.readUInt16LE(12) + header.readUInt16LE(14);
  const table = range(root, offset + RESOURCE_HEADER_BYTES, count * RESOURCE_ENTRY_BYTES);
  return Array.from({ length: count }, (_, index) => ({
    id: table.readUInt32LE(index * RESOURCE_ENTRY_BYTES),
    child: table.readUInt32LE(index * RESOURCE_ENTRY_BYTES + 4),
  }));
}

function resourcePayloads(root, type, map) {
  const entry = entries(root, 0).find((item) => item.id === type);
  requirePe(entry && entry.child & HIGH_BIT, `missing resource type ${type}`);
  const result = [];
  for (const name of entries(root, entry.child & OFFSET_MASK)) {
    requirePe(!(name.id & HIGH_BIT) && name.child & HIGH_BIT, 'invalid resource name');
    for (const language of entries(root, name.child & OFFSET_MASK)) {
      requirePe(!(language.child & HIGH_BIT), 'resource directory depth exceeds three');
      const data = range(root, language.child, 16);
      const size = data.readUInt32LE(4);
      requirePe(size > 0, 'empty resource payload');
      result.push({ id: name.id, bytes: map(data.readUInt32LE(0), size) });
    }
  }
  requirePe(result.length > 0, `empty resource type ${type}`);
  return result;
}

function validateIconGroups(groups, icons) {
  for (const { bytes } of groups) {
    const header = range(bytes, 0, 6);
    const count = header.readUInt16LE(4);
    requirePe(header.readUInt16LE(2) === 1 && count > 0, 'invalid icon group');
    const items = range(bytes, 6, count * 14);
    for (let index = 0; index < count; index += 1) {
      const id = items.readUInt16LE(index * 14 + 12);
      const size = items.readUInt32LE(index * 14 + 8);
      requirePe(
        icons.some((icon) => icon.id === id && icon.bytes.length === size),
        'missing icon image',
      );
    }
  }
}

function validateManifest(manifest) {
  const xml = manifest.replace(/<!--[\s\S]*?-->/g, '');
  requirePe(
    /<assemblyIdentity\b[^>]*\bname=["']com\.hongguo\.nsh["']/.test(xml),
    'wrong manifest identity',
  );
  requirePe(
    /<requestedExecutionLevel\b[^>]*\blevel=["']asInvoker["']/.test(xml),
    'missing asInvoker manifest',
  );
  requirePe(
    /<dpiAwareness\b[^>]*>\s*PerMonitorV2\s*<\/dpiAwareness>/.test(xml),
    'missing DPI manifest',
  );
  requirePe(
    /<longPathAware\b[^>]*>\s*true\s*<\/longPathAware>/.test(xml),
    'missing long-path manifest',
  );
}
