import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { auditWindowsBundle } from './nsh-native-media.mjs';
import { readWindowsImports, validateWindowsPe } from './nsh-windows-pe.mjs';

const PE = 0x80;
const OPTIONAL = PE + 24;
const RAW = 0x200;
const RVA = 0x1000;
const RESOURCE = RAW + 0x200;
const RESOURCE_RVA = RVA + 0x200;
const HIGH_BIT = 0x80000000;

function executable(
  importName = 'kernel32.dll',
  manifest = readFileSync(new URL('../crates/hongguo-nsh/windows.manifest', import.meta.url)),
) {
  const image = Buffer.alloc(8192);
  image.write('MZ');
  image.writeUInt32LE(PE, 60);
  image.writeUInt32LE(0x00004550, PE);
  image.writeUInt16LE(0x8664, PE + 4);
  image.writeUInt16LE(1, PE + 6);
  image.writeUInt16LE(240, PE + 20);
  image.writeUInt16LE(0x0022, PE + 22);
  image.writeUInt16LE(0x20b, OPTIONAL);
  image.writeUInt16LE(2, OPTIONAL + 68);
  image.writeBigUInt64LE(8388608n, OPTIONAL + 72);
  image.writeUInt32LE(16, OPTIONAL + 108);
  image.writeUInt32LE(RVA, OPTIONAL + 120);
  image.writeUInt32LE(40, OPTIONAL + 124);
  image.writeUInt32LE(RESOURCE_RVA, OPTIONAL + 128);
  image.writeUInt32LE(image.length - RESOURCE, OPTIONAL + 132);
  const section = OPTIONAL + 240;
  image.writeUInt32LE(RVA, section + 12);
  image.writeUInt32LE(image.length - RAW, section + 16);
  image.writeUInt32LE(RAW, section + 20);
  image.writeUInt32LE(RVA + 0x40, RAW + 12);
  image.write(importName, RAW + 0x40);

  const icon = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  const group = Buffer.alloc(20);
  group.writeUInt16LE(1, 2);
  group.writeUInt16LE(1, 4);
  group.writeUInt32LE(icon.length, 14);
  group.writeUInt16LE(1, 18);
  image.writeUInt16LE(3, RESOURCE + 14);
  for (const [index, type, bytes] of [
    [0, 3, icon],
    [1, 14, group],
    [2, 24, manifest],
  ]) {
    const directory = 0x40 + index * 0x40;
    const payload = 0x200 + index * 0x200;
    image.writeUInt32LE(type, RESOURCE + 16 + index * 8);
    image.writeUInt32LE(HIGH_BIT + directory, RESOURCE + 20 + index * 8);
    image.writeUInt16LE(1, RESOURCE + directory + 14);
    image.writeUInt32LE(1, RESOURCE + directory + 16);
    image.writeUInt32LE(HIGH_BIT + directory + 24, RESOURCE + directory + 20);
    image.writeUInt16LE(1, RESOURCE + directory + 38);
    image.writeUInt32LE(1033, RESOURCE + directory + 40);
    image.writeUInt32LE(directory + 48, RESOURCE + directory + 44);
    image.writeUInt32LE(RESOURCE_RVA + payload, RESOURCE + directory + 48);
    image.writeUInt32LE(bytes.length, RESOURCE + directory + 52);
    bytes.copy(image, RESOURCE + payload);
  }
  return image;
}

function imageWithImport(name, { dll = false, delayed = false } = {}) {
  const image = executable(delayed ? 'kernel32.dll' : name);
  if (dll) image.writeUInt16LE(0x2022, PE + 22);
  if (delayed) {
    image.writeUInt32LE(RVA + 0x80, OPTIONAL + 216);
    image.writeUInt32LE(64, OPTIONAL + 220);
    image.writeUInt32LE(1, RAW + 0x80);
    image.writeUInt32LE(RVA + 0xc0, RAW + 0x84);
    image.write(name, RAW + 0xc0);
  }
  return image;
}

test('the portable image contains system imports, icon images and a GUI manifest', () => {
  assert.deepEqual(validateWindowsPe(executable()), {
    imports: ['kernel32.dll'],
    iconCount: 1,
    manifestCount: 1,
  });
});

test('portable packaging rejects dynamic C and C++ runtime dependencies', () => {
  for (const name of [
    'MSVCP140.dll',
    'msvcrt.dll',
    'VCRUNTIME140_1.dll',
    'ucrtbase.dll',
    'api-ms-win-crt-heap-l1-1-0.dll',
  ]) {
    assert.throws(() => validateWindowsPe(executable(name)), /dynamic CRT dependency/);
  }
  const multiple = executable();
  multiple.writeUInt32LE(60, OPTIONAL + 124);
  multiple.writeUInt32LE(RVA + 0x80, RAW + 32);
  multiple.write('VCRUNTIME140.dll', RAW + 0x80);
  assert.throws(() => validateWindowsPe(multiple), /dynamic CRT dependency/);
});

test('delay-loaded C runtime dependencies also reject the single-executable package', () => {
  const image = executable();
  image.writeUInt32LE(RVA + 0x80, OPTIONAL + 216);
  image.writeUInt32LE(64, OPTIONAL + 220);
  image.writeUInt32LE(1, RAW + 0x80);
  image.writeUInt32LE(RVA + 0xc0, RAW + 0x84);
  image.write('VCRUNTIME140.dll', RAW + 0xc0);
  assert.throws(() => validateWindowsPe(image), /dynamic CRT dependency/);
});

test('executable and bundled DLL imports accept ordinary Windows system media libraries', () => {
  for (const name of ['mf.dll', 'mfplat.dll', 'mfreadwrite.dll', 'evr.dll']) {
    for (const dll of [false, true]) {
      for (const delayed of [false, true]) {
        assert.deepEqual(
          readWindowsImports(imageWithImport(name, { dll, delayed }), { dll }),
          delayed ? ['kernel32.dll', name] : [name],
        );
      }
    }
  }
});

test('both executable and bundled DLL imports reject WebView2 loaders', () => {
  for (const dll of [false, true]) {
    for (const delayed of [false, true]) {
      assert.throws(
        () => readWindowsImports(imageWithImport('WebView2Loader.dll', { dll, delayed }), { dll }),
        /WebView2 loader/,
      );
    }
  }
  const dll = executable('libde265.dll');
  dll.writeUInt16LE(0x2022, PE + 22);
  assert.deepEqual(readWindowsImports(dll, { dll: true }), ['libde265.dll']);
  assert.throws(() => readWindowsImports(dll), /expected executable image/);
});

test('Windows bundle audits accept system media imports and reject unknown normal or delay imports', (context) => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-pe-bundle-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const application = 'Hongguo.exe';
  const libraries = ['libde265.dll'];
  writeFileSync(join(directory, application), executable());
  for (const name of libraries)
    writeFileSync(join(directory, name), imageWithImport('kernel32.dll', { dll: true }));

  for (const dll of [false, true]) {
    const path = join(directory, dll ? libraries[0] : application);
    for (const delayed of [false, true]) {
      for (const dependency of [
        'mf.dll',
        'MFPLAT.dll',
        'mfreadwrite.dll',
        'mfcore.dll',
        'mfplay.dll',
        'evr.dll',
        'msmpeg2adec.dll',
        'msmpeg2vdec.dll',
        'xaudio2_8.dll',
        'xaudio2_9.dll',
      ]) {
        writeFileSync(path, imageWithImport(dependency, { dll, delayed }));
        assert.doesNotThrow(() => auditWindowsBundle(directory));
      }
      for (const dependency of ['unbundled.dll', 'mfthirdparty.dll', 'xaudio2_8thirdparty.dll']) {
        writeFileSync(path, imageWithImport(dependency, { dll, delayed }));
        assert.throws(
          () => auditWindowsBundle(directory),
          /neither bundled nor approved system DLL/,
        );
      }
    }
    writeFileSync(path, imageWithImport('kernel32.dll', { dll }));
  }
});

test('missing icons, groups, manifest and broken group references cannot pass packaging', () => {
  for (const index of [0, 1, 2]) {
    const image = executable();
    image.writeUInt32LE(99, RESOURCE + 16 + index * 8);
    assert.throws(() => validateWindowsPe(image), /missing resource type/);
  }
  const image = executable();
  image.writeUInt16LE(99, RESOURCE + 0x400 + 18);
  assert.throws(() => validateWindowsPe(image), /missing icon image/);
});

test('a manifest that requests elevation is rejected', () => {
  const image = executable();
  const offset = image.indexOf('asInvoker');
  image.write('requireAd', offset);
  assert.throws(() => validateWindowsPe(image), /missing asInvoker/);
  const commented = readFileSync(
    new URL('../crates/hongguo-nsh/windows.manifest', import.meta.url),
    'utf8',
  )
    .replace('asInvoker', 'requireAdministrator')
    .replace('<trustInfo', '<!-- <requestedExecutionLevel level="asInvoker" /> --><trustInfo');
  assert.throws(
    () => validateWindowsPe(executable('kernel32.dll', Buffer.from(commented))),
    /missing asInvoker/,
  );
});

test('truncated headers, invalid RVAs and cyclic resource directories fail within bounded parsing', () => {
  for (const size of [0, 63, PE + 23, RAW + 10]) {
    assert.throws(() => validateWindowsPe(executable().subarray(0, size)), /Invalid Windows PE/);
  }
  const invalidRva = executable();
  invalidRva.writeUInt32LE(0xffffffff, RAW + 12);
  assert.throws(() => validateWindowsPe(invalidRva), /RVA/);
  const tooManySections = executable();
  tooManySections.writeUInt16LE(97, PE + 6);
  assert.throws(() => validateWindowsPe(tooManySections), /section count/);
  const unterminatedName = executable();
  unterminatedName.fill(65, RAW + 0x40, RAW + 0x40 + 1024);
  assert.throws(() => validateWindowsPe(unterminatedName), /invalid import name/);
  const cycle = executable();
  cycle.writeUInt32LE(HIGH_BIT, RESOURCE + 20);
  assert.throws(() => validateWindowsPe(cycle), /resource directory depth/);
});

test('foreign architecture, console subsystem and reduced stack cannot replace the desktop binary', () => {
  for (const [offset, value] of [
    [PE + 4, 0xaa64],
    [OPTIONAL + 68, 3],
  ]) {
    const image = executable();
    image.writeUInt16LE(value, offset);
    assert.throws(() => validateWindowsPe(image), /Invalid Windows PE/);
  }
  const image = executable();
  image.writeBigUInt64LE(1048576n, OPTIONAL + 72);
  assert.throws(() => validateWindowsPe(image), /8 MiB/);
});
