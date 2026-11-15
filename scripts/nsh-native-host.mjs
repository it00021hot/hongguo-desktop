import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  prepareNativeMedia,
  copyDesktopMedia,
  copyMediaNotices,
  desktopTriple,
  auditWindowsBundle,
  auditMacosBundle,
  verifyNshWorkspace,
} from './nsh-native-media.mjs';
import { runtimeEnvironment, rustTarget } from './native-sdk.mjs';
import { buildAndroidRuntime } from './nsh-android-runtime.mjs';
import { auditAndroidElf, verifyAndroidApk } from './nsh-android-audit.mjs';
import { relinkEnvironment, finishRelink, copyAndroidRelinkHost } from './nsh-relink.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const metadata = JSON.parse(readFileSync(join(root, 'crates/hongguo-nsh/nsh.json'), 'utf8'));
const [action, target, profile = 'debug', requestedTriple = ''] = process.argv.slice(2);
const release = profile === 'release';
const cargoTarget = resolve(root, process.env.CARGO_TARGET_DIR ?? 'target');
const xml = (value) =>
  value
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;');
function run(program, args, env = {}, cwd = root) {
  const result = spawnSync(program, args, {
    cwd,
    env: { ...process.env, ...env },
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${program} exited with ${result.status}`);
}
function output(program, args) {
  const result = spawnSync(program, args, { cwd: root, encoding: 'utf8' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${program}: ${result.stderr}`);
  return result.stdout.trim();
}
function sources(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? sources(path) : entry.name.endsWith('.java') ? [path] : [];
  });
}
function buildAndroid() {
  const triple = 'aarch64-linux-android';
  const sdk = process.env.ANDROID_HOME;
  const ndk = process.env.ANDROID_NDK_HOME ?? process.env.NDK_HOME;
  if (!sdk || !ndk) throw new Error('ANDROID_HOME and ANDROID_NDK_HOME are required');
  const distribution = process.env.NSH_DISTRIBUTION;
  if (!distribution)
    throw new Error('NSH_DISTRIBUTION must name the verified NSH embedding SDK installation');
  const aar = join(distribution, 'embed/android/nsh.aar');
  const host = { win32: 'windows-x86_64', darwin: 'darwin-x86_64', linux: 'linux-x86_64' }[
    process.platform
  ];
  if (!host) throw new Error(`Unsupported build host: ${process.platform}`);
  const bin = join(ndk, 'toolchains/llvm/prebuilt', host, 'bin');
  const suffix = process.platform === 'win32' ? '.exe' : '';
  const clang = join(bin, `clang${suffix}`);
  const prefix = prepareNativeMedia(triple);
  const out = join(root, 'target/nsh-native/android');
  const relink = join(out, 'Relink');
  const flags = ['--target=aarch64-linux-android28'];
  const env = {
    ...runtimeEnvironment(prefix),
    ANDROID_NDK_ROOT: ndk,
    NSH_ANDROID_MIN_SDK: '28',
    CMAKE_TOOLCHAIN_FILE_aarch64_linux_android: join(root, 'scripts/nsh-android.cmake'),
    CC_aarch64_linux_android: clang,
    CXX_aarch64_linux_android: join(bin, `clang++${suffix}`),
    AR_aarch64_linux_android: join(bin, `llvm-ar${suffix}`),
    CFLAGS_aarch64_linux_android: flags.join(' '),
    CXXFLAGS_aarch64_linux_android: flags.join(' '),
    BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android: `--sysroot="${join(bin, '../sysroot')}" ${flags.join(' ')}`,
    ...relinkEnvironment(triple, prefix, clang, ndk, relink, [...flags, '-Wl,--no-undefined']),
  };
  run(
    'cargo',
    ['build', '-p', 'hongguo-nsh-android', '--target', triple, ...(release ? ['--release'] : [])],
    env,
  );
  const binary = join(cargoTarget, triple, profile, 'libhongguo_nsh_android.so');
  finishRelink(binary, triple, relink);
  auditAndroidElf(binary, join(bin, `llvm-readelf${suffix}`));
  const runtime = join(out, 'runtime');
  rmSync(runtime, { recursive: true, force: true });
  mkdirSync(join(runtime, 'lib/arm64-v8a'), { recursive: true });
  copyFileSync(binary, join(runtime, 'lib/arm64-v8a/libnsh.so'));
  buildAndroidRuntime(
    {
      aar,
      sdk,
      output: out,
      sourceDirectory: join(root, 'vendor/nsh/crates/nsh-platform-android/java'),
    },
    run,
  );
  const platform = JSON.parse(
    readFileSync(join(out, 'runtime-source.json'), 'utf8'),
  ).androidJarPath;
  if (!existsSync(platform)) throw new Error(`Android SDK 37 is required: ${platform}`);
  const java = join(root, 'crates/hongguo-nsh-android/android/app/src/main');
  const classes = join(out, 'host-classes');
  rmSync(classes, { recursive: true, force: true });
  mkdirSync(classes, { recursive: true });
  const jar = join(out, 'runtime-java/classes.jar');
  const args = [
    '--release',
    '17',
    '-encoding',
    'UTF-8',
    '-classpath',
    [platform, jar].join(process.platform === 'win32' ? ';' : ':'),
    '-d',
    classes,
    ...sources(join(java, 'java')),
  ];
  const response = join(out, 'host-javac.args');
  writeFileSync(
    response,
    args.map((value) => JSON.stringify(value.replaceAll('\\', '/'))).join('\n'),
  );
  run('javac', [`@${response}`]);
  const hostJar = join(out, 'host.jar');
  run('jar', ['cf', hostJar, '-C', classes, '.']);
  const tools = readdirSync(join(sdk, 'build-tools')).sort((a, b) =>
    b.localeCompare(a, undefined, { numeric: true }),
  )[0];
  run('java', [
    '-cp',
    join(sdk, 'build-tools', tools, 'lib/d8.jar'),
    'com.android.tools.r8.D8',
    '--lib',
    platform,
    '--min-api',
    '28',
    '--output',
    runtime,
    jar,
    hostJar,
  ]);
  cpSync(join(java, 'res'), join(runtime, 'res'), { recursive: true });
  cpSync(join(java, 'assets'), join(runtime, 'assets'), { recursive: true });
  for (const name of ['LICENSE', 'NOTICE', 'THIRD-PARTY-NOTICES.md'])
    copyFileSync(join(root, name), join(runtime, 'assets/licenses', name));
  for (const [directory, name] of [
    ['values', metadata.name],
    ['values-zh', metadata.nativeHost.displayNames['zh-CN']],
  ]) {
    mkdirSync(join(runtime, 'res', directory), { recursive: true });
    writeFileSync(
      join(runtime, 'res', directory, 'strings.xml'),
      `<resources><string name="app_name">${xml(name)}</string></resources>`,
    );
  }
  mkdirSync(join(runtime, 'res/drawable'), { recursive: true });
  copyFileSync(
    join(root, 'assets/icons/512x512.png'),
    join(runtime, 'res/drawable/guava_icon.png'),
  );
  const manifest = readFileSync(join(java, 'AndroidManifest.xml'), 'utf8')
    .replace('<manifest ', `<manifest package="${metadata.id}" `)
    .replace('<application ', '<uses-sdk android:minSdkVersion="28" /><application ')
    .replace('android:name=".MainActivity"', 'android:name="com.hongguo.nsh.MainActivity"');
  writeFileSync(join(runtime, 'AndroidManifest.xml'), manifest);
  copyMediaNotices(prefix, triple, join(out, 'NativeMedia'));
  copyAndroidRelinkHost(relink, out);
}
function build() {
  verifyNshWorkspace();
  if (target === 'android') return buildAndroid();
  const mobile = ['ios', 'ios-simulator'].includes(target);
  const triple =
    requestedTriple ||
    (mobile
      ? target === 'ios'
        ? 'aarch64-apple-ios'
        : 'aarch64-apple-ios-sim'
      : desktopTriple(target));
  const prefix = prepareNativeMedia(triple);
  const env = runtimeEnvironment(prefix);
  if (target === 'macos') env.MACOSX_DEPLOYMENT_TARGET = '14.0';
  const cargo = [
    'build',
    '-p',
    mobile ? 'hongguo-nsh-ios' : 'hongguo-nsh',
    ...(release ? ['--release'] : []),
  ];
  const targeted = Boolean(requestedTriple || mobile || !release);
  if (targeted) cargo.push('--target', triple);
  if (target === 'windows' && release)
    cargo.push('--config', join(root, 'crates/hongguo-nsh/windows-build.toml'));
  if (mobile) {
    const sdkName = target === 'ios' ? 'iphoneos' : 'iphonesimulator';
    const sdk = output('xcrun', ['--sdk', sdkName, '--show-sdk-path']);
    const linker = output('xcrun', ['--sdk', sdkName, '--find', 'clang']);
    Object.assign(
      env,
      relinkEnvironment(
        triple,
        prefix,
        linker,
        sdk,
        join(root, 'target/nsh-native', target, 'Relink'),
      ),
      { SDKROOT: sdk, IPHONEOS_DEPLOYMENT_TARGET: '16.0' },
    );
  }
  run('cargo', cargo, env);
  const built = join(
    cargoTarget,
    ...(targeted ? [triple] : []),
    profile,
    mobile ? 'hongguo-nsh-ios' : `hongguo-nsh${target === 'windows' ? '.exe' : ''}`,
  );
  const stable = join(root, 'target/nsh-native', target, profile);
  mkdirSync(stable, { recursive: true });
  copyFileSync(built, join(stable, target === 'windows' ? 'host.exe' : 'host'));
  if (target === 'windows')
    copyFileSync(join(prefix, 'bin/libde265.dll'), join(stable, 'libde265.dll'));
  if (mobile) {
    const out = join(root, 'target/nsh-native', target);
    finishRelink(built, triple, join(out, 'Relink'));
    copyMediaNotices(prefix, triple, join(out, 'NativeMedia'));
  }
}
function stage() {
  const destination = resolve(profile);
  const executable = requestedTriple;
  if (target === 'android') {
    for (const folder of ['Relink', 'NativeMedia'])
      cpSync(join(root, 'target/nsh-native/android', folder), join(destination, folder), {
        recursive: true,
      });
    const apk = join(destination, executable);
    if (executable.endsWith('.apk'))
      verifyAndroidApk(
        apk,
        join(root, 'target/nsh-native/android/runtime/lib/arm64-v8a/libnsh.so'),
        process.env.ANDROID_HOME,
        run,
      );
    return;
  }
  if (target.startsWith('ios')) {
    for (const folder of ['Relink', 'NativeMedia'])
      cpSync(join(root, 'target/nsh-native', target, folder), join(dirname(destination), folder), {
        recursive: true,
      });
  } else {
    const arch =
      target === 'macos'
        ? output('lipo', ['-archs', join(destination, 'Contents/MacOS', executable)])
        : '';
    if (arch.includes(' '))
      throw new Error(
        'Universal media libraries are not configured; package each architecture separately',
      );
    const triple = desktopTriple(
      target,
      arch === 'x86_64' ? 'x64' : arch === 'arm64' ? 'arm64' : process.arch,
    );
    copyDesktopMedia(prepareNativeMedia(triple), triple, destination);
    if (target === 'windows') {
      auditWindowsBundle(destination);
      return;
    } else {
      const binary = join(destination, 'Contents/MacOS', executable);
      const commands = output('otool', ['-l', binary]);
      for (const match of commands.matchAll(/cmd LC_RPATH[\s\S]*?\n\s*path (.+?) \(offset/g)) {
        if (match[1].includes('/native-media/'))
          run('install_name_tool', ['-delete_rpath', match[1], binary]);
      }
      auditMacosBundle(destination, output);
    }
  }
  const localized =
    target === 'macos'
      ? join(destination, 'Contents/Resources/zh-Hans.lproj')
      : join(destination, 'zh-Hans.lproj');
  mkdirSync(localized, { recursive: true });
  writeFileSync(
    join(localized, 'InfoPlist.strings'),
    `"CFBundleDisplayName" = ${JSON.stringify(metadata.nativeHost.displayNames['zh-CN'])};\n`,
  );
}
if (action === 'build') build();
else if (action === 'stage') stage();
else if (action === 'setup') prepareNativeMedia(target || rustTarget());
else throw new Error('Expected build, stage or setup');
