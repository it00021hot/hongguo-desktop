import { createHash } from 'node:crypto';
import {
  existsSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { basename, dirname, join, relative, resolve, sep } from 'node:path';
import { libraries, source } from './native-media-config.mjs';

const WINDOWS_SYSTEM_DLL =
  /^(?:kernel32|advapi32|bcrypt|ole32|oleaut32|user32|gdi32|dxgi|d3d11|ws2_32|secur32|shell32|ntdll|psapi|api-ms-win-core-[\w-]+)\.dll$/i;
const MEDIA_LIBRARY =
  /^(?:lib)?(?:avcodec|avutil|swresample|avfilter)[.-]\d+(?:[.\d]*)\.(?:dylib|dll)$/;

export function validateDependency(spec, dependency) {
  const isSystem =
    spec.os === 'macos'
      ? dependency.startsWith('/usr/lib/') || dependency.startsWith('/System/Library/')
      : WINDOWS_SYSTEM_DLL.test(dependency);
  const isBundled =
    spec.os === 'macos'
      ? dependency.startsWith('@rpath/') && MEDIA_LIBRARY.test(basename(dependency))
      : MEDIA_LIBRARY.test(dependency);
  if (!isSystem && !isBundled) throw new Error(`Unexpected runtime dependency: ${dependency}`);
}

export function inspectLibraries(prefix, spec, run, toolchain) {
  const directory = join(prefix, spec.os === 'windows' ? 'bin' : 'lib');
  const extension = spec.linkage === 'static' ? '.a' : spec.os === 'windows' ? '.dll' : '.dylib';
  const paths = readdirSync(directory).filter((name) => name.endsWith(extension));
  for (const library of libraries) {
    if (
      !paths.some((name) => name.startsWith(`lib${library}.`) || name.startsWith(`${library}-`))
    ) {
      throw new Error(`Missing FFmpeg library: ${library} in ${directory}`);
    }
  }
  return [...new Set(paths.map((name) => realpathSync(join(directory, name))))].map((path) => {
    const architecture = inspectArchitecture(path, spec, run, toolchain);
    const dependencies = runtimeDependencies(path, spec, run, toolchain);
    for (const dependency of dependencies) validateDependency(spec, dependency);
    if (
      !architecture.includes(
        spec.arch === 'aarch64'
          ? spec.os === 'android'
            ? 'AArch64'
            : 'arm64'
          : spec.os === 'windows'
            ? '8664'
            : 'x86_64',
      )
    ) {
      throw new Error(`Wrong native media architecture: ${path}\n${architecture}`);
    }
    return {
      path: relative(prefix, path),
      architecture: architecture.trim().replaceAll(path, relative(prefix, path)),
      dependencies,
    };
  });
}

function inspectArchitecture(path, spec, run, toolchain) {
  if (spec.os === 'windows')
    return toolchain.readobj
      ? run(toolchain.readobj, ['--file-header', path])
      : run('dumpbin', ['/headers', path]);
  if (spec.os === 'android') {
    const readelf = join(
      dirname(toolchain.compiler),
      `llvm-readelf${process.platform === 'win32' ? '.exe' : ''}`,
    );
    return [
      ...new Set(
        run(readelf, ['-h', path])
          .split('\n')
          .filter((line) => line.includes('Machine:')),
      ),
    ].join('\n');
  }
  if (spec.linkage === 'static') return run('xcrun', ['lipo', '-archs', path]);
  const build = run('xcrun', ['vtool', '-show-build', path]);
  if (!new RegExp(`minos ${spec.minimumOs.replaceAll('.', '\\.')}\\b`).test(build))
    throw new Error(`Unexpected minimum OS: ${path}\n${build}`);
  return `${run('file', [path]).trim()}\n${build}`;
}

function runtimeDependencies(path, spec, run, toolchain) {
  if (spec.linkage === 'static') return [];
  if (spec.os === 'windows') {
    if (toolchain.readobj)
      return [
        ...run(toolchain.readobj, ['--coff-imports', path]).matchAll(
          /^\s+Name: ([\w.-]+\.dll)$/gim,
        ),
      ].map((match) => match[1]);
    return run('dumpbin', ['/dependents', path])
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => /^[\w.-]+\.dll$/i.test(line));
  }
  return run('otool', ['-L', path])
    .split('\n')
    .slice(1)
    .map((line) => line.trim().split(' (')[0])
    .filter(Boolean);
}

export function writeManifest(prefix, metadata) {
  const artifacts = inventory(prefix);
  const manifest = { schemaVersion: 1, ...metadata, artifacts };
  writeFileSync(join(prefix, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

function inventory(prefix, directory = prefix) {
  return readdirSync(directory, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) return inventory(prefix, path);
      if (entry.name === 'manifest.json') return [];
      const resolved = realpathSync(path);
      if (!resolved.startsWith(`${realpathSync(prefix)}${sep}`))
        throw new Error(`Native media symlink escapes prefix: ${path}`);
      return [
        {
          path: relative(prefix, path).replaceAll('\\', '/'),
          sha256: digest(path),
          byteLength: statSync(path).size,
        },
      ];
    });
}

function digest(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

export function verifyInstall(prefix, spec) {
  const manifest = JSON.parse(readFileSync(join(prefix, 'manifest.json'), 'utf8'));
  if (
    manifest.schemaVersion !== 1 ||
    manifest.source.sha256 !== source.sha256 ||
    manifest.target.triple !== spec.triple
  ) {
    throw new Error(`Native media manifest does not match the locked source/target: ${prefix}`);
  }
  if (!manifest.artifacts.length) throw new Error('Native media manifest contains no artifacts');
  for (const artifact of manifest.artifacts) {
    const path = resolve(prefix, artifact.path);
    if (
      !path.startsWith(`${resolve(prefix)}${sep}`) ||
      !existsSync(path) ||
      !realpathSync(path).startsWith(`${realpathSync(prefix)}${sep}`) ||
      statSync(path).size !== artifact.byteLength ||
      digest(path) !== artifact.sha256
    ) {
      throw new Error(`Native media artifact is missing or modified: ${artifact.path}`);
    }
  }
  return manifest;
}
