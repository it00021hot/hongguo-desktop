import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, realpathSync } from 'node:fs';
import { delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildIos } from './nsh-ios.mjs';
import { buildDesktop } from './nsh-desktop.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const app = join(root, 'crates/hongguo-nsh');
const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
const [action = 'run', target] = process.argv.slice(2);
const ANDROID_MIN_SDK = 28;

function run(command, args, env = {}, cwd = root) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: 'inherit',
    env: { ...process.env, ...env },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
}

function requirePath(path, description) {
  if (!path || !existsSync(path))
    throw new Error(`${description} is required: ${path ?? 'not configured'}`);
  return path;
}

function distribution() {
  if (process.env.NSH_DISTRIBUTION)
    return requirePath(process.env.NSH_DISTRIBUTION, 'NSH_DISTRIBUTION');
  const binary = (process.env.PATH ?? '')
    .split(delimiter)
    .map((directory) => join(directory, process.platform === 'win32' ? 'nsh.exe' : 'nsh'))
    .find(existsSync);
  if (!binary) throw new Error('NSH 0.4.0 is required');
  return dirname(dirname(realpathSync(binary)));
}

function android() {
  const sdk = requirePath(process.env.ANDROID_HOME, 'ANDROID_HOME');
  const ndk = requirePath(process.env.NDK_HOME, 'NDK_HOME');
  const host = { darwin: 'darwin-x86_64', linux: 'linux-x86_64', win32: 'windows-x86_64' }[
    process.platform
  ];
  if (!host) throw new Error(`Unsupported Android build host: ${process.platform}`);
  const toolchain = requirePath(
    join(ndk, 'toolchains/llvm/prebuilt', host, 'bin'),
    'NDK LLVM toolchain',
  );
  const suffix = process.platform === 'win32' ? '.cmd' : '';
  const linker = requirePath(
    join(toolchain, `aarch64-linux-android${ANDROID_MIN_SDK}-clang${suffix}`),
    'Android clang',
  );
  const archiver = requirePath(
    join(toolchain, process.platform === 'win32' ? 'llvm-ar.exe' : 'llvm-ar'),
    'Android archiver',
  );
  const environment = {
    ANDROID_HOME: sdk,
    NSH_ANDROID_MIN_SDK: String(ANDROID_MIN_SDK),
    ANDROID_NDK_ROOT: ndk,
    CMAKE_TOOLCHAIN_FILE_aarch64_linux_android: join(root, 'scripts/nsh-android.cmake'),
    CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER: linker,
    CC_aarch64_linux_android: linker,
    CXX_aarch64_linux_android: join(
      toolchain,
      `aarch64-linux-android${ANDROID_MIN_SDK}-clang++${suffix}`,
    ),
    AR_aarch64_linux_android: archiver,
    BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android: `--sysroot="${join(toolchain, '../sysroot').replaceAll('\\', '/')}" --target=aarch64-linux-android${ANDROID_MIN_SDK}`,
    SYSTEM_DEPS_LIBVVDEC_BUILD_INTERNAL: 'always',
    PKG_CONFIG_ALLOW_CROSS_aarch64_linux_android: '1',
    PKG_CONFIG_LIBDIR_aarch64_linux_android: '',
    SYSTEM_DEPS_LIBVVDEC_LIB: 'vvdec c++_static c++abi m dl unwind',
  };
  run(
    'cargo',
    ['build', '-p', 'hongguo-nsh-android', '--target', 'aarch64-linux-android', '--release'],
    environment,
  );
  const output = join(root, 'target/nsh-android');
  const drawable = join(output, 'resources/drawable');
  mkdirSync(drawable, { recursive: true });
  copyFileSync(
    join(root, 'crates/hongguo-tauri-app/icons/512x512.png'),
    join(drawable, 'hongguo_icon.png'),
  );
  const libraries = join(output, 'jniLibs/arm64-v8a');
  mkdirSync(libraries, { recursive: true });
  copyFileSync(
    join(root, 'target/aarch64-linux-android/release/libnsh.so'),
    join(libraries, 'libnsh.so'),
  );
  const runtime = join(output, 'nsh-runtime.aar');
  const unpacked = join(output, 'runtime-java');
  mkdirSync(unpacked, { recursive: true });
  const aar = requirePath(join(distribution(), 'embed/android/nsh.aar'), 'NSH Android runtime');
  const entries = ['classes.jar', 'AndroidManifest.xml', 'R.txt', 'proguard.txt'];
  run('jar', ['xf', aar, ...entries], {}, unpacked);
  run('jar', ['cf', runtime, ...entries], {}, unpacked);
  const project = join(root, 'crates/hongguo-nsh-android/android');
  const wrapper = join(project, process.platform === 'win32' ? 'gradlew.bat' : 'gradlew');
  if (process.platform === 'win32') {
    run(
      'cmd.exe',
      [
        '/d',
        '/c',
        wrapper,
        `-PappVersion=${version}`,
        `-PappMinSdk=${ANDROID_MIN_SDK}`,
        'assembleDebug',
      ],
      environment,
      project,
    );
  } else {
    run(
      'sh',
      [wrapper, `-PappVersion=${version}`, `-PappMinSdk=${ANDROID_MIN_SDK}`, 'assembleDebug'],
      environment,
      project,
    );
  }
}

function ios() {
  buildIos(target);
}

function preview() {
  const platform = { darwin: 'macos', win32: 'windows' }[process.platform];
  if (!platform) throw new Error('Layout preview currently requires macOS or Windows');
  requirePath(
    join(root, 'target/nsh-layout-fixtures/player-frame.png'),
    'Local layout preview frame',
  );
  run('nsh', ['test', join(app, 'preview/layout'), '--target', platform], {
    CARGO_TARGET_DIR: join(app, 'preview/target'),
  });
}

try {
  if (action === 'check')
    run('cargo', [
      'check',
      '-p',
      'hongguo-nsh',
      '-p',
      'hongguo-nsh-android',
      '-p',
      'hongguo-nsh-ios',
      '--all-targets',
    ]);
  else if (action === 'test') {
    run(process.execPath, [
      '--test',
      'scripts/nsh-desktop.test.mjs',
      'scripts/nsh-ios.test.mjs',
      'scripts/nsh-windows-adapters.test.mjs',
      'scripts/nsh-windows-pe.test.mjs',
    ]);
    run('cargo', ['test', '-p', 'hongguo-nsh', '--lib']);
  } else if (action === 'preview') preview();
  else if (action === 'run') run('cargo', ['run', '-p', 'hongguo-nsh']);
  else if (action === 'package' && target === 'android') android();
  else if (action === 'package' && ['ios', 'ios-simulator'].includes(target)) ios();
  else if (action === 'package' && ['windows', 'macos'].includes(target)) {
    console.log(buildDesktop(target));
  } else
    throw new Error(
      'Usage: node scripts/nsh.mjs run|check|test|preview|package macos|windows|android|ios|ios-simulator',
    );
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
