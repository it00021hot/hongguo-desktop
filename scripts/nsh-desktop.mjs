import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateWindowsPe } from './nsh-windows-pe.mjs';
import { runtimeEnvironment } from './native-sdk.mjs';
import { buildWindowsInstaller } from './nsh-windows-installer.mjs';
import {
  auditMacosBundle,
  auditWindowsBundle,
  copyDesktopMedia,
  desktopTriple,
  prepareNativeMedia,
  validateNshSource,
} from './nsh-native-media.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const MAX_CARGO_OUTPUT_BYTES = 64 * 1024 * 1024;

export function buildDesktop(target) {
  const host = { darwin: 'macos', win32: 'windows' }[process.platform];
  if (!host || target !== host) throw new Error(`Desktop packaging requires a ${target} host`);
  const dependencies = run('cargo', [
    'tree',
    '-p',
    'hongguo-nsh',
    '--edges',
    'normal',
    '--prefix',
    'none',
  ]);
  if (/^(?:nsh-js|quickjs[^ ]*) v/m.test(dependencies))
    throw new Error('The pure Rust desktop host must not include QuickJS');
  validateNshSource(JSON.parse(run('cargo', ['metadata', '--format-version=1'])));
  const triple = desktopTriple(target);
  const prefix = prepareNativeMedia(triple, run);
  const build = run(
    'cargo',
    [
      'build',
      ...(target === 'windows'
        ? ['--config', join(root, 'crates/hongguo-nsh/windows-build.toml')]
        : []),
      '-p',
      'hongguo-nsh',
      '--bin',
      'hongguo-nsh',
      '--release',
      '--message-format=json-render-diagnostics',
    ],
    {
      ...runtimeEnvironment(prefix),
      ...(target === 'macos' ? { MACOSX_DEPLOYMENT_TARGET: '14.0' } : {}),
    },
  );
  const executable = desktopExecutable(build);
  if (target === 'macos') validateMacos(executable);
  else validateWindowsPe(readFileSync(executable));
  const output = join(root, 'target/nsh-desktop', target);
  mkdirSync(output, { recursive: true });
  if (target === 'macos') return macos(executable, output, prefix, triple);
  const destination = join(output, 'Hongguo');
  rmSync(destination, { recursive: true, force: true });
  mkdirSync(destination, { recursive: true });
  copyFileSync(executable, join(destination, 'Hongguo.exe'));
  copyDesktopMedia(prefix, triple, destination);
  const inspection = auditWindowsBundle(destination);
  writeFileSync(
    join(destination, 'NativeMedia/runtime-inspection.json'),
    JSON.stringify(inspection, null, 2),
  );
  const installer = buildWindowsInstaller(destination);
  run(
    'powershell.exe',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      "$ErrorActionPreference = 'Stop'; Compress-Archive -LiteralPath $env:HONGGUO_NSH_PACKAGE_DIR -DestinationPath $env:HONGGUO_NSH_PACKAGE_ZIP -Force",
    ],
    {
      HONGGUO_NSH_PACKAGE_DIR: destination,
      HONGGUO_NSH_PACKAGE_ZIP: join(output, 'Hongguo.zip'),
    },
  );
  return installer;
}

export function desktopExecutable(build) {
  const artifacts = build
    .trim()
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line));
  const executables = artifacts.filter(
    (artifact) =>
      artifact.reason === 'compiler-artifact' &&
      artifact.target?.name === 'hongguo-nsh' &&
      artifact.target.kind.includes('bin') &&
      typeof artifact.executable === 'string',
  );
  if (executables.length !== 1 || !isAbsolute(executables[0].executable))
    throw new Error('Cargo must report exactly one desktop executable');
  return executables[0].executable;
}

export function validateMacosPlatform(commands) {
  const platforms = [...commands.matchAll(/cmd LC_BUILD_VERSION[\s\S]*?^\s*platform (\d+)\s*$/gm)];
  if (platforms.length !== 1 || platforms[0][1] !== '1')
    throw new Error('Cargo did not build for the macOS platform');
}

function validateMacos(executable) {
  const header = run('otool', ['-hv', executable]);
  if (!/^MH_MAGIC_64[^\n]*\bEXECUTE\b/m.test(header))
    throw new Error('Cargo did not build a macOS executable');
  validateMacosPlatform(run('otool', ['-l', executable]));
  const dependencies = run('otool', ['-L', executable]);
  for (const line of dependencies.split('\n').slice(1)) {
    const dependency = line.trim().split(' (compatibility version ')[0];
    if (!dependency) continue;
    if (
      !dependency.startsWith('/System/Library/') &&
      !dependency.startsWith('/usr/lib/') &&
      !/^@rpath\/libde265(?:\.\d+)*\.dylib$/.test(dependency)
    )
      throw new Error(`Unexpected macOS runtime dependency: ${dependency}`);
  }
}

function macos(binary, output, prefix, triple) {
  const destination = join(output, '红果.app');
  rmSync(destination, { recursive: true, force: true });
  const contents = join(destination, 'Contents');
  const executable = join(contents, 'MacOS/hongguo-nsh');
  mkdirSync(dirname(executable), { recursive: true });
  mkdirSync(join(contents, 'Resources'), { recursive: true });
  copyFileSync(binary, executable);
  copyDesktopMedia(prefix, triple, destination);
  const rpaths = [
    ...run('otool', ['-l', executable]).matchAll(/cmd LC_RPATH[\s\S]*?\n\s*path (.+?) \(offset/g),
  ].map((match) => match[1]);
  for (const path of rpaths) {
    if (path.includes('/native-media/'))
      run('install_name_tool', ['-delete_rpath', path, executable]);
  }
  copyFileSync(
    join(root, 'crates/hongguo-tauri-app/icons/icon.icns'),
    join(contents, 'Resources/AppIcon.icns'),
  );
  const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  const plist = join(contents, 'Info.plist');
  writeFileSync(
    plist,
    JSON.stringify({
      CFBundleIdentifier: 'com.hongguo.nsh',
      CFBundleExecutable: 'hongguo-nsh',
      CFBundleName: '红果',
      CFBundleDisplayName: '红果',
      CFBundlePackageType: 'APPL',
      CFBundleShortVersionString: version,
      CFBundleVersion: version,
      CFBundleIconFile: 'AppIcon.icns',
      LSMinimumSystemVersion: '14.0',
      LSApplicationCategoryType: 'public.app-category.video',
      NSHighResolutionCapable: true,
      NSAppTransportSecurity: {
        NSExceptionDomains: {
          '127.0.0.1': { NSExceptionAllowsInsecureHTTPLoads: true },
        },
      },
    }),
  );
  run('plutil', ['-convert', 'xml1', plist]);
  for (const library of auditMacosBundle(destination, run))
    run('codesign', ['--force', '--sign', '-', '--timestamp=none', library]);
  run('codesign', ['--force', '--sign', '-', '--timestamp=none', destination]);
  run('codesign', ['--verify', '--deep', '--strict', destination]);
  run('ditto', ['-c', '-k', '--keepParent', destination, join(output, 'Hongguo.zip')]);
  return destination;
}

function run(command, args, environment = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    env: { ...process.env, ...environment },
    encoding: 'utf8',
    maxBuffer: MAX_CARGO_OUTPUT_BYTES,
    stdio: command === 'cargo' && args[0] === 'build' ? ['inherit', 'pipe', 'inherit'] : 'pipe',
  });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`${command} exited with ${result.status}: ${result.stderr ?? ''}`);
  return result.stdout ?? '';
}
