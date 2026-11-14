import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from 'node:fs';
import { delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildIos } from './nsh-ios.mjs';
import { buildDesktop } from './nsh-desktop.mjs';
import { copyMediaNotices, prepareNativeMedia, verifyNshWorkspace } from './nsh-native-media.mjs';
import { copyAndroidRelinkHost, finishRelink, relinkEnvironment } from './nsh-relink.mjs';
import { auditAndroidElf, verifyAndroidApk } from './nsh-android-audit.mjs';
import { buildAndroidRuntime } from './nsh-android-runtime.mjs';
import { runtimeEnvironment, rustTarget } from './native-sdk.mjs';

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
  if (!binary) throw new Error('NSH 0.5.0 is required');
  return dirname(dirname(realpathSync(binary)));
}

function android() {
  const aar = requirePath(
    join(distribution(), 'embed/android/nsh.aar'),
    'NSH 0.5.0 Android embedding SDK (install nsh-embed-android.zip from NSH v0.5.0)',
  );
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
  const linker = requirePath(
    join(
      toolchain,
      process.platform === 'win32' ? 'clang.exe' : `aarch64-linux-android${ANDROID_MIN_SDK}-clang`,
    ),
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
      process.platform === 'win32'
        ? 'clang++.exe'
        : `aarch64-linux-android${ANDROID_MIN_SDK}-clang++`,
    ),
    AR_aarch64_linux_android: archiver,
    BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android: `--sysroot="${join(toolchain, '../sysroot').replaceAll('\\', '/')}" --target=aarch64-linux-android${ANDROID_MIN_SDK}`,
  };
  const linkerArgs =
    process.platform === 'win32' ? [`--target=aarch64-linux-android${ANDROID_MIN_SDK}`] : [];
  if (linkerArgs.length) {
    environment.CFLAGS_aarch64_linux_android = linkerArgs.join(' ');
    environment.CXXFLAGS_aarch64_linux_android = linkerArgs.join(' ');
  }
  verifyNshWorkspace();
  const output = join(root, 'target/nsh-android');
  buildAndroidRuntime(
    {
      aar,
      sdk,
      output,
      sourceDirectory: join(root, 'vendor/nsh/crates/nsh-platform-android/java'),
    },
    run,
  );
  environment.HONGGUO_HEVC_ROOT = prepareNativeMedia('aarch64-linux-android');
  const relink = join(output, 'Relink');
  Object.assign(
    environment,
    relinkEnvironment(
      'aarch64-linux-android',
      environment.HONGGUO_HEVC_ROOT,
      linker,
      ndk,
      relink,
      linkerArgs,
    ),
  );
  run(
    'cargo',
    ['build', '-p', 'hongguo-nsh-android', '--target', 'aarch64-linux-android', '--release'],
    environment,
  );
  finishRelink(
    join(root, 'target/aarch64-linux-android/release/libhongguo_nsh_android.so'),
    'aarch64-linux-android',
    relink,
  );
  copyMediaNotices(
    environment.HONGGUO_HEVC_ROOT,
    'aarch64-linux-android',
    join(output, 'NativeMedia'),
  );
  copyFileSync(
    join(output, 'runtime-source.json'),
    join(output, 'NativeMedia/NSH-runtime-source.json'),
  );
  const nativeBinary = join(root, 'target/aarch64-linux-android/release/libhongguo_nsh_android.so');
  const inspection = auditAndroidElf(
    nativeBinary,
    join(toolchain, process.platform === 'win32' ? 'llvm-readelf.exe' : 'llvm-readelf'),
  );
  writeFileSync(
    join(output, 'NativeMedia/runtime-inspection.json'),
    JSON.stringify(inspection, null, 2),
  );
  const drawable = join(output, 'resources/drawable');
  mkdirSync(drawable, { recursive: true });
  copyFileSync(
    join(root, 'crates/hongguo-tauri-app/icons/512x512.png'),
    join(drawable, 'hongguo_icon.png'),
  );
  const libraries = join(output, 'jniLibs/arm64-v8a');
  mkdirSync(libraries, { recursive: true });
  copyFileSync(
    join(root, 'target/aarch64-linux-android/release/libhongguo_nsh_android.so'),
    join(libraries, 'libnsh.so'),
  );
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
        'clean',
        'assembleDebug',
      ],
      environment,
      project,
    );
  } else {
    run(
      'sh',
      [
        wrapper,
        `-PappVersion=${version}`,
        `-PappMinSdk=${ANDROID_MIN_SDK}`,
        'clean',
        'assembleDebug',
      ],
      environment,
      project,
    );
  }
  copyAndroidRelinkHost(relink, output);
  copyFileSync(
    join(project, 'app/build/outputs/apk/debug/app-debug.apk'),
    join(output, 'Hongguo.apk'),
  );
  verifyAndroidApk(join(output, 'Hongguo.apk'), nativeBinary, sdk, run);
  const archive = join(root, 'target/Hongguo-android.zip');
  run('jar', [
    'cf',
    archive,
    '-C',
    output,
    'Hongguo.apk',
    '-C',
    output,
    'NativeMedia',
    '-C',
    output,
    'Relink',
  ]);
}

function ios() {
  buildIos(target);
}

function preview(environment) {
  const platform = { darwin: 'macos', win32: 'windows' }[process.platform];
  if (!platform) throw new Error('Layout preview currently requires macOS or Windows');
  const cli = spawnSync('nsh', ['--version'], { encoding: 'utf8' });
  if (cli.error) throw cli.error;
  if (cli.status !== 0 || !/^nsh 0\.5\.0\b/.test(cli.stdout.trim()))
    throw new Error('Preview requires the matching NSH 0.5.0 CLI');
  requirePath(
    join(root, 'target/nsh-layout-fixtures/player-frame.png'),
    'Local layout preview frame',
  );
  run('nsh', ['test', join(app, 'preview/layout'), '--target', platform], {
    ...environment,
    CARGO_TARGET_DIR: join(app, 'preview/target'),
  });
}

try {
  if (['run', 'check', 'test', 'preview'].includes(action)) verifyNshWorkspace();
  const hostAction = ['run', 'check', 'test', 'setup', 'preview'].includes(action);
  const environment = hostAction
    ? runtimeEnvironment(
        prepareNativeMedia(action === 'setup' && target ? target : rustTarget(), run),
      )
    : {};
  if (action === 'check')
    run(
      'cargo',
      [
        'check',
        '-p',
        'hongguo-nsh',
        '-p',
        'hongguo-nsh-android',
        '-p',
        'hongguo-nsh-ios',
        '--all-targets',
      ],
      environment,
    );
  else if (action === 'test') {
    run(process.execPath, [
      '--test',
      'scripts/nsh-android.test.mjs',
      'scripts/nsh-android-audit.test.mjs',
      'scripts/nsh-android-runtime.test.mjs',
      'scripts/nsh-desktop.test.mjs',
      'scripts/nsh-ios.test.mjs',
      'scripts/nsh-windows-adapters.test.mjs',
      'scripts/nsh-windows-pe.test.mjs',
      'scripts/native-media-relink.test.mjs',
      'scripts/nsh-native-media.test.mjs',
      'scripts/native-sdk.test.mjs',
      'scripts/native-sdk-package.test.mjs',
    ]);
    run('cargo', ['test', '-p', 'hongguo-nsh', '--lib'], environment);
  } else if (action === 'preview') preview(environment);
  else if (action === 'run') run('cargo', ['run', '-p', 'hongguo-nsh'], environment);
  else if (action === 'setup')
    console.log(`Native SDK ready: ${environment.HONGGUO_NATIVE_SDK_ROOT}`);
  else if (action === 'package' && target === 'android') android();
  else if (action === 'package' && ['ios', 'ios-simulator'].includes(target)) ios();
  else if (action === 'package' && ['windows', 'macos'].includes(target)) {
    console.log(buildDesktop(target));
  } else
    throw new Error(
      'Usage: node scripts/nsh.mjs run|check|test|preview|setup [Rust target]|package macos|windows|android|ios|ios-simulator',
    );
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
