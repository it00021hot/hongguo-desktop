import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
} from 'node:fs';
import { join, sep } from 'node:path';
import { hevcSource } from './native-codec-build.mjs';
import {
  validateBuildEvidence,
  validateHevcInspection,
  vvdecTarget,
} from './native-codec-evidence.mjs';
import { digest, inventorySdk, jsonDigest, run, writeJson } from './native-codec-io.mjs';
import { readWindowsImports } from './nsh-windows-pe.mjs';

export const vvdecSource = Object.freeze({
  version: '3.2.1',
  sha256: '5c334557a33cd93e981b84ba0e77126ef970a38e2481b67f0475db40f75379b8',
});

export function stageVvdec(repository, destination, target, build) {
  if (build) return stageBuiltVvdec(repository, destination, target, build);
  const input = join(
    repository,
    'target/native-media',
    target.triple,
    'vvdec-' + vvdecSource.version,
  );
  const manifest = JSON.parse(readFileSync(join(input, 'manifest.json'), 'utf8'));
  if (
    manifest.source?.version !== vvdecSource.version ||
    manifest.source?.sha256 !== vvdecSource.sha256 ||
    manifest.target?.triple !== target.triple
  )
    throw new Error('VVdeC input source or target mismatch');
  const source = inventorySdk(join(repository, 'crates/hongguo-vvdec-sys/vendor/vvdec'));
  if (jsonDigest(source) !== manifest.source.vendoredTreeSha256)
    throw new Error('VVdeC source tree differs from the native library build');
  for (const patch of manifest.source.patches ?? []) {
    if (
      !['private-frame-allocator.patch', 'picture-pool.cmake'].includes(patch.path) ||
      digest(join(repository, 'crates/hongguo-vvdec-sys', patch.path)) !== patch.sha256
    )
      throw new Error('VVdeC allocator recipe differs from the native library build');
  }
  const actual = new Map(inventorySdk(input).map((artifact) => [artifact.path, artifact]));
  for (const artifact of manifest.artifacts) {
    const file = actual.get(artifact.path);
    if (!file || file.sha256 !== artifact.sha256 || file.byteLength !== artifact.byteLength)
      throw new Error('VVdeC input artifact modified: ' + artifact.path);
  }
  const evidence = JSON.parse(readFileSync(join(input, manifest.build.inspectionPath), 'utf8'));
  validateBuildEvidence(vvdecTarget(target.triple), evidence);
  cpSync(input, destination, { recursive: true, dereference: false, verbatimSymlinks: true });
  return manifest;
}

function stageBuiltVvdec(repository, destination, target, prefix) {
  if (target.triple !== 'aarch64-apple-darwin' || process.platform !== 'darwin')
    throw new Error('VVdeC allocator SDK maintenance requires the native Apple Silicon host');
  const directory = join(repository, 'crates/hongguo-vvdec-sys');
  const source = join(directory, 'vendor/vvdec');
  const inherited = join(
    repository,
    'target/native-media',
    target.triple,
    'vvdec-' + vvdecSource.version,
  );
  const original = JSON.parse(readFileSync(join(inherited, 'manifest.json'), 'utf8'));
  const inventory = inventorySdk(source);
  if (jsonDigest(inventory) !== original.source.vendoredTreeSha256)
    throw new Error('VVdeC upstream tree differs from the authenticated SDK source');
  const build = join(prefix, 'build');
  const cache = readFileSync(join(build, 'CMakeCache.txt'), 'utf8');
  const cached = (name) => {
    const value = cache.match(new RegExp('^' + name + ':[^=]+=(.*)$', 'm'))?.[1];
    if (!value) throw new Error('VVdeC build cache is missing ' + name);
    return value;
  };
  if (
    cached('CMAKE_BUILD_TYPE') !== 'Release' ||
    realpathSync(cached('CMAKE_HOME_DIRECTORY')) !== realpathSync(source) ||
    realpathSync(cached('CMAKE_PROJECT_INCLUDE')) !==
      realpathSync(join(directory, 'picture-pool.cmake')) ||
    realpathSync(cached('HONGGUO_PICTURE_POOL_SOURCE')) !==
      realpathSync(join(prefix, 'PicListManager.cpp'))
  )
    throw new Error('VVdeC source build does not match the pinned allocator recipe');
  const info = join(destination, 'build-info');
  const patches = join(destination, 'patches');
  mkdirSync(info, { recursive: true });
  mkdirSync(patches);
  const patched = join(info, 'PicListManager.cpp');
  copyFileSync(join(source, 'source/Lib/CommonLib/PicListManager.cpp'), patched);
  const patch = join(directory, 'private-frame-allocator.patch');
  run('/usr/bin/patch', ['--batch', '--forward', patched, patch]);
  if (digest(patched) !== digest(join(prefix, 'PicListManager.cpp')))
    throw new Error('VVdeC compiled source differs from the allocator patch');
  run('cmake', ['--build', build, '--target', 'install', '--config', 'Release', '--clean-first']);
  const library = join(prefix, 'lib/libvvdec.a');
  const inspection = run(tool('llvm-readobj'), ['--file-headers', '--macho-version-min', library]);
  const arches = [...new Set([...inspection.matchAll(/^Arch: (.+)$/gm)].map((value) => value[1]))];
  const platforms = [
    ...new Set([...inspection.matchAll(/^\s+Platform: (.+)$/gm)].map((value) => value[1])),
  ];
  const flags = readFileSync(
    join(build, 'source/Lib/vvdec/CMakeFiles/vvdec.dir/flags.make'),
    'utf8',
  );
  const evidence = {
    configuration: 'Release',
    sourceMatches: true,
    genericFlags: flags.match(/^CXX_FLAGS = (.*)$/m)?.[1],
    architecture: arches.length === 1 && arches[0] === 'aarch64' ? 'arm64' : arches.join(','),
    platforms: platforms.map((value) => (value === 'macos' ? '1' : value)),
    minimumVersions: [
      ...new Set([...inspection.matchAll(/^\s+Version: (.+)$/gm)].map((value) => value[1])),
    ],
  };
  if (!/(?:^|\s)-mcpu=apple-m1(?:\s|$)/.test(evidence.genericFlags ?? ''))
    throw new Error('VVdeC Apple Silicon objects must use the M1 CPU baseline');
  validateBuildEvidence(vvdecTarget(target.triple), evidence);
  cpSync(join(prefix, 'include'), join(destination, 'include'), { recursive: true });
  mkdirSync(join(destination, 'lib'));
  copyFileSync(library, join(destination, 'lib/libvvdec.a'));
  copyFileSync(join(prefix, 'bindings.rs'), join(destination, 'bindings.rs'));
  cpSync(join(inherited, 'licenses'), join(destination, 'licenses'), { recursive: true });
  for (const name of ['LICENSE.txt', 'AUTHORS.md'])
    copyFileSync(join(source, name), join(destination, 'licenses', name));
  copyFileSync(join(directory, 'SOURCE.md'), join(destination, 'SOURCE.md'));
  copyFileSync(patch, join(patches, 'private-frame-allocator.patch'));
  copyFileSync(join(directory, 'picture-pool.cmake'), join(patches, 'picture-pool.cmake'));
  copyFileSync(join(build, 'CMakeCache.txt'), join(info, 'CMakeCache.txt'));
  copyFileSync(
    join(build, 'source/Lib/vvdec/CMakeFiles/vvdec.dir/flags.make'),
    join(info, 'compiler-configuration.txt'),
  );
  writeJson(join(info, 'library-inspection.json'), evidence);
  writeJson(join(info, 'source-inventory.json'), inventory);
  const manifest = {
    schemaVersion: 1,
    source: {
      ...vvdecSource,
      vendoredTreeSha256: jsonDigest(inventory),
      patches: inventorySdk(patches),
    },
    target: { ...vvdecTarget(target.triple), cpuBaseline: 'apple-m1' },
    bindingsPath: 'bindings.rs',
    libraryPath: 'lib/libvvdec.a',
    build: {
      provenance: 'cargo-source-build',
      cmakeCacheSha256: digest(join(info, 'CMakeCache.txt')),
      flagsSha256: digest(join(info, 'compiler-configuration.txt')),
      inspectionPath: 'build-info/library-inspection.json',
    },
    artifacts: inventorySdk(destination),
  };
  writeJson(join(destination, 'manifest.json'), manifest);
  return manifest;
}

function tool(name) {
  const directory = process.env.HONGGUO_INSPECTION_LLVM_BIN;
  if (directory) return join(directory, name);
  const local = '/opt/homebrew/opt/llvm/bin/' + name;
  return existsSync(local) ? local : name;
}

function checkedInput(root, relative) {
  const canonical = realpathSync(root);
  const path = realpathSync(join(root, relative));
  if (!path.startsWith(canonical + sep) || !statSync(path).isFile())
    throw new Error('Native build input escapes its prefix: ' + relative);
  return path;
}

export function stageHevc(repository, destination, target) {
  const pointer = JSON.parse(
    readFileSync(join(repository, 'target/native-codec-build', target.triple + '.json'), 'utf8'),
  );
  const prefix = pointer.install;
  const evidence = JSON.parse(readFileSync(join(prefix, 'build-evidence.json'), 'utf8'));
  if (
    evidence.source.sha256 !== hevcSource.sha256 ||
    evidence.source.commit !== hevcSource.commit ||
    evidence.target.triple !== target.triple ||
    evidence.target.crt !== target.crt
  )
    throw new Error('HEVC build source or target mismatch');
  if (digest(evidence.sourceArchive) !== hevcSource.sha256)
    throw new Error('HEVC corresponding source archive changed');
  const commands = JSON.parse(
    readFileSync(join(evidence.buildDirectory, 'compile_commands.json'), 'utf8'),
  );
  const library =
    target.os === 'windows'
      ? 'lib/de265.lib'
      : target.os === 'macos'
        ? 'lib/libde265.dylib'
        : 'lib/libde265.a';
  const runtime =
    target.os === 'windows'
      ? 'bin/libde265.dll'
      : target.os === 'macos'
        ? 'lib/libde265.0.dylib'
        : undefined;
  const image = checkedInput(prefix, runtime ?? library);
  const inspection = run(tool('llvm-readobj'), [
    '--file-headers',
    ...(target.os === 'macos' || target.os === 'ios' ? ['--macho-version-min'] : []),
    image,
  ]);
  validateHevcInspection(target, inspection, commands);
  const imports =
    target.os === 'windows' ? readWindowsImports(readFileSync(image), { dll: true }) : [];
  mkdirSync(destination, { recursive: true });
  cpSync(join(prefix, 'include'), join(destination, 'include'), { recursive: true });
  for (const directory of ['bin', 'lib']) {
    const source = join(prefix, directory);
    if (!existsSync(source)) continue;
    mkdirSync(join(destination, directory), { recursive: true });
    for (const file of readdirSync(source)) {
      if (!/\.(?:dll|lib|a|dylib)$/.test(file)) continue;
      checkedInput(source, file);
      cpSync(join(source, file), join(destination, directory, file), {
        dereference: false,
        verbatimSymlinks: true,
      });
    }
  }
  for (const directory of ['sources', 'licenses', 'build-info'])
    mkdirSync(join(destination, directory), { recursive: true });
  copyFileSync(evidence.sourceArchive, join(destination, 'sources/libde265-1.1.3.tar.gz'));
  for (const name of ['COPYING', 'AUTHORS'])
    copyFileSync(join(evidence.sourceDirectory, name), join(destination, 'licenses', name));
  copyFileSync(
    join(prefix, 'build-evidence.json'),
    join(destination, 'build-info/build-evidence.json'),
  );
  copyFileSync(
    join(evidence.buildDirectory, 'CMakeCache.txt'),
    join(destination, 'build-info/CMakeCache.txt'),
  );
  copyFileSync(
    join(evidence.buildDirectory, 'compile_commands.json'),
    join(destination, 'build-info/compile_commands.json'),
  );
  writeJson(join(destination, 'build-info/library-inspection.json'), { inspection, imports });
  const manifest = {
    schemaVersion: 1,
    source: hevcSource,
    target,
    libraryPath: library,
    ...(runtime ? { runtimePath: runtime } : {}),
    build: { options: evidence.options, compiler: evidence.compiler },
    artifacts: inventorySdk(destination),
  };
  writeJson(join(destination, 'manifest.json'), manifest);
  return manifest;
}
