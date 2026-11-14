import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { copyMediaNotices, prepareNativeMedia, verifyNshWorkspace } from './nsh-native-media.mjs';
import { finishRelink, relinkEnvironment } from './nsh-relink.mjs';
import { profileEntitlements } from './native-media-resign-ios.mjs';
export { profileEntitlements } from './native-media-resign-ios.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const minimumVersion = '16.0';
const bundleId = 'com.hongguo.nsh';

export function buildIos(target) {
  if (process.platform !== 'darwin') throw new Error('iOS builds require macOS and full Xcode');
  if (!['ios', 'ios-simulator'].includes(target)) throw new Error('Expected ios or ios-simulator');
  const simulator = target === 'ios-simulator';
  const rustTarget = simulator ? 'aarch64-apple-ios-sim' : 'aarch64-apple-ios';
  verifyNshWorkspace();
  const prefix = prepareNativeMedia(rustTarget, run);
  const sdk = output('xcrun', [
    '--sdk',
    simulator ? 'iphonesimulator' : 'iphoneos',
    '--show-sdk-path',
  ]);
  const relink = join(root, 'target/nsh-ios', simulator ? 'simulator' : 'device', 'Relink');
  const linker = output('xcrun', [
    '--sdk',
    simulator ? 'iphonesimulator' : 'iphoneos',
    '--find',
    'clang',
  ]);
  run('cargo', ['build', '-p', 'hongguo-nsh-ios', '--target', rustTarget, '--release'], {
    ...relinkEnvironment(rustTarget, prefix, linker, sdk, relink),
    IPHONEOS_DEPLOYMENT_TARGET: minimumVersion,
    SDKROOT: sdk,
    [`BINDGEN_EXTRA_CLANG_ARGS_${rustTarget}`]: `-isysroot "${sdk}" --target=arm64-apple-ios${minimumVersion}${simulator ? '-simulator' : ''}`,
  });
  finishRelink(join(root, 'target', rustTarget, 'release/hongguo-nsh-ios'), rustTarget, relink);
  const destination = createBundle(rustTarget, simulator);
  copyMediaNotices(prefix, rustTarget, join(dirname(destination), 'NativeMedia'));
  signBundle(destination, simulator);
  run('ditto', [
    '-c',
    '-k',
    '--keepParent',
    dirname(destination),
    join(root, 'target/nsh-ios', `Hongguo-${simulator ? 'simulator' : 'device'}.zip`),
  ]);
  return destination;
}

function createBundle(rustTarget, simulator) {
  const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
  const destination = join(
    root,
    'target/nsh-ios',
    simulator ? 'simulator' : 'device',
    'Hongguo.app',
  );
  mkdirSync(destination, { recursive: true });
  copyFileSync(
    join(root, 'target', rustTarget, 'release/hongguo-nsh-ios'),
    join(destination, 'Hongguo'),
  );
  writeFileSync(
    join(destination, 'Info.plist'),
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>${bundleId}</string>
<key>CFBundleExecutable</key><string>Hongguo</string>
<key>CFBundleName</key><string>Hongguo</string>
<key>CFBundleDisplayName</key><string>Hongguo</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>${version}</string>
<key>CFBundleVersion</key><string>${version}</string>
<key>CFBundleSupportedPlatforms</key><array><string>${simulator ? 'iPhoneSimulator' : 'iPhoneOS'}</string></array>
<key>MinimumOSVersion</key><string>${minimumVersion}</string>
<key>LSRequiresIPhoneOS</key><true/>
<key>CADisableMinimumFrameDurationOnPhone</key><true/>
<key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
<key>UISupportedInterfaceOrientations</key><array><string>UIInterfaceOrientationPortrait</string><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string></array>
<key>UISupportedInterfaceOrientations~ipad</key><array><string>UIInterfaceOrientationPortrait</string><string>UIInterfaceOrientationPortraitUpsideDown</string><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string></array>
<key>UILaunchScreen</key><dict/>
<key>UIApplicationSceneManifest</key><dict><key>UIApplicationSupportsMultipleScenes</key><false/></dict>
<key>NSAppTransportSecurity</key><dict><key>NSExceptionDomains</key><dict>
<key>127.0.0.1</key><dict><key>NSExceptionAllowsInsecureHTTPLoads</key><true/></dict>
</dict></dict>
</dict></plist>`,
  );
  const localized = join(destination, 'zh-Hans.lproj');
  mkdirSync(localized, { recursive: true });
  writeFileSync(join(localized, 'InfoPlist.strings'), '"CFBundleDisplayName" = "红果";\n');
  compileIcon(destination, simulator);
  run('plutil', ['-lint', join(destination, 'Info.plist')]);
  return destination;
}

function compileIcon(destination, simulator) {
  const catalog = join(dirname(destination), 'Assets.xcassets');
  const icon = join(catalog, 'AppIcon.appiconset');
  mkdirSync(icon, { recursive: true });
  copyFileSync(
    join(root, 'crates/hongguo-tauri-app/icons/1024x1024.png'),
    join(icon, 'AppIcon.png'),
  );
  writeFileSync(
    join(catalog, 'Contents.json'),
    JSON.stringify({ info: { author: 'xcode', version: 1 } }),
  );
  writeFileSync(
    join(icon, 'Contents.json'),
    JSON.stringify({
      images: [{ filename: 'AppIcon.png', idiom: 'universal', platform: 'ios', size: '1024x1024' }],
      info: { author: 'xcode', version: 1 },
    }),
  );
  const partial = join(dirname(destination), 'icon-info.plist');
  run('xcrun', [
    'actool',
    catalog,
    '--compile',
    destination,
    '--app-icon',
    'AppIcon',
    '--platform',
    simulator ? 'iphonesimulator' : 'iphoneos',
    '--minimum-deployment-target',
    minimumVersion,
    '--target-device',
    'iphone',
    '--target-device',
    'ipad',
    '--output-partial-info-plist',
    partial,
    '--output-format',
    'human-readable-text',
  ]);
  const path = join(destination, 'Info.plist');
  const info = JSON.parse(output('plutil', ['-convert', 'json', '-o', '-', path]));
  const generated = JSON.parse(output('plutil', ['-convert', 'json', '-o', '-', partial]));
  writeFileSync(path, JSON.stringify({ ...info, ...generated }));
  run('plutil', ['-convert', 'xml1', path]);
}

function signBundle(destination, simulator) {
  if (simulator) {
    run('codesign', ['--force', '--sign', '-', destination]);
  } else {
    const identity = process.env.NSH_IOS_SIGN_IDENTITY;
    if (!identity)
      throw new Error(
        `Unsigned app built at ${destination}; NSH_IOS_SIGN_IDENTITY and NSH_IOS_PROFILE are required for device signing`,
      );
    const entitlements = prepareProfile(destination);
    run('codesign', [
      '--force',
      '--sign',
      identity,
      '--entitlements',
      entitlements,
      '--generate-entitlement-der',
      '--timestamp=none',
      destination,
    ]);
  }
  run('codesign', ['--verify', '--strict', destination]);
}

function prepareProfile(destination) {
  const profile = process.env.NSH_IOS_PROFILE;
  if (!profile || !existsSync(profile))
    throw new Error('NSH_IOS_PROFILE must name an existing provisioning profile');
  const directory = dirname(destination);
  const entitlements = profileEntitlements(readProfile(profile));
  const path = join(directory, 'entitlements.plist');
  writeFileSync(path, JSON.stringify(entitlements), { mode: 0o600 });
  run('plutil', ['-convert', 'xml1', path]);
  copyFileSync(profile, join(destination, 'embedded.mobileprovision'));
  return path;
}

function readProfile(profile) {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-nsh-sign-'));
  const decoded = join(directory, 'profile.plist');
  try {
    run('security', ['cms', '-D', '-i', profile, '-o', decoded]);
    return {
      expiresAt: output('plutil', ['-extract', 'ExpirationDate', 'raw', '-o', '-', decoded]),
      prefix: JSON.parse(
        output('plutil', ['-extract', 'ApplicationIdentifierPrefix', 'json', '-o', '-', decoded]),
      )[0],
      granted: JSON.parse(
        output('plutil', ['-extract', 'Entitlements', 'json', '-o', '-', decoded]),
      ),
    };
  } finally {
    rmSync(directory, { recursive: true });
  }
}

function run(command, args, environment = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env, ...environment },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
}

function output(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
  return result.stdout.trim();
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    buildIos(process.argv[2]);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
