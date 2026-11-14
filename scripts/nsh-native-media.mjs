import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  writeFileSync,
} from 'node:fs';
import { spawnSync } from 'node:child_process';
import { basename, dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { source, targetSpec } from './native-media-config.mjs';
import { verifyInstall } from './native-media-manifest.mjs';
import { readWindowsImports, validateWindowsPe } from './nsh-windows-pe.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SYSTEM_DLLS = new Set(
  `advapi32 avrt bcrypt bcryptprimitives cfgmgr32 combase comctl32 comdlg32 crypt32 cryptbase
  d2d1 d3d11 d3d12 d3dcompiler_47 dcomp dnsapi dwrite dwmapi dxgi gdi32 imm32 iphlpapi
  kernel32 kernelbase msimg32 msvfw32 ncrypt netapi32 ntdll ole32 oleacc oleaut32 opengl32
  powrprof propsys psapi rpcrt4 secur32 setupapi shell32 shcore shlwapi uiautomationcore
  user32 userenv usp10 uxtheme version winhttp wininet winmm winspool ws2_32 wtsapi32
  wintrust windowscodecs normaliz hid dbghelp`.split(/\s+/),
);

export function desktopTriple(platform, architecture = process.arch) {
  if (platform === 'macos' && ['arm64', 'x64'].includes(architecture))
    return `${architecture === 'arm64' ? 'aarch64' : 'x86_64'}-apple-darwin`;
  if (platform === 'windows' && architecture === 'x64') return 'x86_64-pc-windows-msvc';
  throw new Error(`Unsupported desktop media target: ${platform}/${architecture}`);
}

export function prepareNativeMedia(triple, run) {
  const script = join(root, 'scripts/native-media.mjs');
  run(process.execPath, [script, 'build', triple]);
  run(process.execPath, [script, 'verify', triple]);
  const prefix = join(root, 'target/native-media', triple, `ffmpeg-${source.version}`);
  verifyInstall(prefix, targetSpec(triple));
  return prefix;
}

export function copyMediaNotices(prefix, triple, destination) {
  const manifest = verifyInstall(prefix, targetSpec(triple));
  mkdirSync(destination, { recursive: true });
  for (const directory of ['licenses', 'sources', 'build-info'])
    cpSync(join(prefix, directory), join(destination, 'ffmpeg', directory), { recursive: true });
  copyFileSync(join(prefix, 'manifest.json'), join(destination, 'ffmpeg/source-manifest.json'));
  const vvdec = join(root, 'crates/hongguo-vvdec-sys');
  const notices = join(destination, 'vvdec');
  mkdirSync(notices, { recursive: true });
  for (const name of ['LICENSE.txt', 'AUTHORS.md'])
    copyFileSync(join(vvdec, 'vendor/vvdec', name), join(notices, name));
  copyFileSync(join(vvdec, 'SOURCE.md'), join(notices, 'SOURCE.md'));
  copyFileSync(
    join(vvdec, 'vendor/vvdec/thirdparty/simde/COPYING'),
    join(notices, 'SIMDe-COPYING'),
  );
  copyFileSync(join(root, 'vendor/nsh/LICENSE-APACHE'), join(destination, 'NSH-LICENSE-APACHE'));
  const anime4k = join(destination, 'anime4k');
  mkdirSync(anime4k, { recursive: true });
  for (const name of ['LICENSE.md', 'graph.json'])
    copyFileSync(
      join(root, 'crates/hongguo-enhancement/shaders/anime4k', name),
      join(anime4k, name),
    );
  if (triple.includes('android')) {
    const ndk = process.env.NDK_HOME ?? process.env.ANDROID_NDK_ROOT ?? manifest.toolchain.sdk;
    const runtime = join(destination, 'android-runtime');
    mkdirSync(runtime, { recursive: true });
    for (const name of ['NOTICE', 'NOTICE.toolchain', 'source.properties'])
      copyFileSync(join(ndk, name), join(runtime, name));
    cpSync(
      join(root, 'vendor/nsh/crates/nsh-platform-android/java'),
      join(destination, 'nsh-android-java'),
      { recursive: true },
    );
  }
  cpSync(join(root, 'scripts'), join(destination, 'build-scripts'), {
    recursive: true,
    filter: (path) =>
      path === join(root, 'scripts') ||
      basename(path).startsWith('native-media') ||
      basename(path) === 'nsh-android-runtime.mjs',
  });
  copyFileSync(join(root, 'docs/native-media-build.md'), join(destination, 'BUILD.md'));
  writeFileSync(
    join(destination, 'README.txt'),
    `FFmpeg ${source.version}: LGPL-2.1-or-later. VVdeC: separate Clear BSD license.\n` +
      'FFmpeg corresponding source, patches, configuration and source artifact hashes: ffmpeg/.\n' +
      'The source manifest describes the build prefix before bundle relocation/signing.\n' +
      'Desktop: replace all four ABI-compatible FFmpeg libraries in the application runtime directory.\n' +
      'macOS: libraries are in Contents/Frameworks. Re-sign each replaced dylib, then the app:\n' +
      '  codesign --force --sign - --timestamp=none <dylib>\n' +
      '  codesign --force --sign - --timestamp=none <app>\n' +
      'A modified app needs a signing identity/profile authorized by its installer on Apple devices.\n' +
      'Mobile static packages require the companion relinking material; see Relink/README.md.\n',
  );
}

export function copyDesktopMedia(prefix, triple, destination) {
  const manifest = verifyInstall(prefix, targetSpec(triple));
  const windows = triple.includes('windows');
  const runtime = windows ? destination : join(destination, 'Contents/Frameworks');
  const notices = windows
    ? join(destination, 'NativeMedia')
    : join(destination, 'Contents/Resources/NativeMedia');
  mkdirSync(runtime, { recursive: true });
  for (const artifact of manifest.artifacts) {
    if (!new RegExp(windows ? '^bin/[^/]+\\.dll$' : '^lib/[^/]+\\.dylib$').test(artifact.path))
      continue;
    cpSync(join(prefix, artifact.path), join(runtime, basename(artifact.path)), {
      dereference: false,
      verbatimSymlinks: true,
    });
  }
  copyMediaNotices(prefix, triple, notices);
  return runtime;
}

export function auditWindowsBundle(destination) {
  const names = readdirSync(destination);
  const images = names.filter((name) => /\.(?:exe|dll)$/i.test(name));
  if (images.filter((name) => /\.exe$/i.test(name)).length !== 1)
    throw new Error('Windows bundle must contain one application executable');
  const dlls = images.filter((name) => /\.dll$/i.test(name));
  if (
    dlls.length !== 4 ||
    !dlls.every((name) => /^(?:avcodec|avutil|avfilter|swresample)-\d+\.dll$/.test(name))
  )
    throw new Error('Windows bundle must contain exactly the four pinned FFmpeg DLLs');
  const available = new Set(images.map((name) => name.toLowerCase()));
  const inspection = [];
  for (const name of images) {
    const bytes = readFileSync(join(destination, name));
    const dll = /\.dll$/i.test(name);
    const imports = dll ? readWindowsImports(bytes, { dll }) : validateWindowsPe(bytes).imports;
    for (const dependency of imports) {
      const normalized = dependency.toLowerCase();
      if (
        !available.has(normalized) &&
        !SYSTEM_DLLS.has(normalized.replace(/\.dll$/, '')) &&
        !/^api-ms-win-(?:core|security|eventing|service|shell|shcore|ntuser|power)-[\w-]+\.dll$/.test(
          normalized,
        )
      )
        throw new Error(
          `Windows runtime dependency is neither bundled nor approved system DLL: ${name} -> ${dependency}`,
        );
    }
    inspection.push({ name, imports });
  }
  return inspection;
}

export function auditMacosBundle(destination, run) {
  const frameworks = join(destination, 'Contents/Frameworks');
  const executable = join(destination, 'Contents/MacOS/hongguo-nsh');
  const libraries = [
    ...new Set(readdirSync(frameworks).map((name) => realpathSync(join(frameworks, name)))),
  ];
  if (libraries.length !== 4) throw new Error('macOS bundle must contain four FFmpeg dylibs');
  for (const path of [executable, ...libraries]) {
    for (const line of run('otool', ['-L', path]).split('\n').slice(1)) {
      const dependency = line.trim().split(' (')[0];
      if (
        !dependency ||
        dependency.startsWith('/System/Library/') ||
        dependency.startsWith('/usr/lib/')
      )
        continue;
      if (
        !dependency.startsWith('@rpath/') ||
        basename(dependency) !== dependency.slice(7) ||
        !existsSync(join(frameworks, basename(dependency)))
      )
        throw new Error(`macOS runtime dependency is not bundled: ${path} -> ${dependency}`);
    }
  }
  return libraries;
}

export function validateNshSource(metadata) {
  const packages = metadata.packages.filter((entry) => entry.name.startsWith('nsh-'));
  if (!packages.length) throw new Error('Cargo metadata contains no NSH packages');
  const expected = realpathSync(join(root, 'vendor/nsh'));
  for (const entry of packages) {
    if (entry.source !== null || !realpathSync(entry.manifest_path).startsWith(`${expected}${sep}`))
      throw new Error(`NSH must resolve to the vendored source: ${entry.name}`);
  }
}

export function verifyNshWorkspace() {
  const result = spawnSync('cargo', ['metadata', '--format-version=1'], {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Cargo source verification failed: ${result.stderr}`);
  validateNshSource(JSON.parse(result.stdout));
}
