import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export function relinkEnvironment(triple, prefix, linker, sdk, destination, linkerArgs = []) {
  return {
    HONGGUO_HEVC_ROOT: prefix,
    HONGGUO_RELINK_REAL_LINKER: linker,
    HONGGUO_RELINK_REAL_LINKER_ARGS: JSON.stringify(linkerArgs),
    HONGGUO_RELINK_SDK_ROOT: sdk,
    HONGGUO_RELINK_OUTPUT: destination,
    HONGGUO_RELINK_TARGET: triple,
    [`CARGO_TARGET_${triple.replaceAll('-', '_').toUpperCase()}_LINKER`]: join(
      root,
      'scripts',
      process.platform === 'win32' ? 'native-media-link.cmd' : 'native-media-link.cjs',
    ),
  };
}

export function finishRelink(binary, triple, destination) {
  const manifest = JSON.parse(readFileSync(join(destination, 'link.json'), 'utf8'));
  const sha256 = createHash('sha256').update(readFileSync(binary)).digest('hex');
  const linked =
    manifest.linkedOutputPath &&
    createHash('sha256').update(readFileSync(manifest.linkedOutputPath)).digest('hex');
  if (manifest.target !== triple || linked !== sha256)
    throw new Error('Relinking materials do not match the actual application binary');
  manifest.linkerOutputSha256 ??= manifest.outputSha256;
  manifest.outputSha256 = sha256;
  writeFileSync(join(destination, 'link.json'), JSON.stringify(manifest, null, 2));
  const source = join(destination, 'BuildSource');
  mkdirSync(source, { recursive: true });
  for (const file of ['Cargo.toml', 'Cargo.lock', 'LICENSE'])
    if (existsSync(join(root, file))) copyFileSync(join(root, file), join(source, file));
  copyFileSync(
    join(root, 'scripts/native-media-resign-ios.mjs'),
    join(destination, 'resign-ios.mjs'),
  );
  copyFileSync(join(root, 'scripts/native-media-relink.mjs'), join(destination, 'relink.mjs'));
  const rustc = spawnSync(process.env.RUSTC ?? 'rustc', ['--print', 'sysroot'], {
    encoding: 'utf8',
  });
  if (rustc.error) throw rustc.error;
  if (rustc.status !== 0) throw new Error('Cannot record Rust standard library notices');
  const notices = join(rustc.stdout.trim(), 'share/doc/rust');
  mkdirSync(join(destination, 'RustNotices'), { recursive: true });
  for (const file of ['COPYRIGHT-library.html', 'licenses']) {
    if (!existsSync(join(notices, file))) throw new Error(`Rust license material missing: ${file}`);
    cpSync(join(notices, file), join(destination, 'RustNotices', file), { recursive: true });
  }
  const ios = triple.includes('apple-ios');
  writeFileSync(
    join(destination, 'README.md'),
    `# Replacing the statically linked libde265 library\n\n` +
      `Target: ${triple}. The recorded link.json and SHA-256 inventory contain the actual final-link inputs.\n` +
      `The application binary before bundle signing has SHA-256 ${sha256}.\n\n` +
      'objects/ contains application/Rust/native link inputs; libde265/lib/ contains the original HEVC archive.\n' +
      'Exact codec source, build configuration and licenses accompany these materials in ../NativeMedia/.\n' +
      'The target SDK/NDK and its compiler are development tools; they are not required on the customer device and are not redistributed here.\n' +
      'Use a compatible target toolchain (the original compiler/SDK paths are recorded), then run:\n\n' +
      '```sh\nnode relink.mjs --linker <target-clang> --sdk <Apple-SDK-or-Android-NDK> --hevc <modified-libde265-prefix> --output <new-binary>\n```\n\n' +
      'Omit --hevc to use the included original archive. A modified prefix must provide lib/libde265.a and a compatible public ABI.\n' +
      'Every captured original object/archive is checked before linking; substituting your modified HEVC prefix is intentionally permitted.\n\n' +
      (ios
        ? 'Copy the supplied app bundle to your working directory. Simulator:\n\n```sh\nnode resign-ios.mjs --app <Hongguo.app> --binary <new-binary> --identity -\n```\n\nFor a device, use your authorized signing identity and provisioning profile:\n\n```sh\nnode resign-ios.mjs --app <Hongguo.app> --binary <new-binary> --identity <identity> --profile <profile.mobileprovision>\n```\n\nThe helper validates the app identifier/profile, replaces only its executable, then signs and verifies the bundle.\n'
        : "To build an APK with your modified library, set --output to AndroidSource/target/nsh-android/jniLibs/arm64-v8a/libnsh.so, then use the included standalone Android host:\n\n```sh\ncd AndroidSource/crates/hongguo-nsh-android/android\n./gradlew -PappVersion=<original-version> -PappMinSdk=28 clean assembleDebug\n```\n\nOn Windows use gradlew.bat. Gradle uses the developer machine's own debug signing key; no private signing key is distributed. For your production identity, configure Android's standard release signing in this copied host project.\n"),
  );
}

export function copyAndroidRelinkHost(destination, output) {
  const snapshot = join(destination, 'AndroidSource');
  rmSync(snapshot, { recursive: true, force: true });
  const project = join(root, 'crates/hongguo-nsh-android/android');
  cpSync(project, join(snapshot, 'crates/hongguo-nsh-android/android'), {
    recursive: true,
    filter: (path) => !['.gradle', 'build', 'local.properties'].includes(basename(path)),
  });
  const artifacts = join(snapshot, 'target/nsh-android');
  mkdirSync(artifacts, { recursive: true });
  for (const name of ['jniLibs', 'resources', 'nsh-runtime.aar', 'runtime-source.json'])
    cpSync(join(output, name), join(artifacts, name), { recursive: true });
}
