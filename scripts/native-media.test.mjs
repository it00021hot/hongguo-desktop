import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { configuration, resolveToolchain, source, targetSpec } from './native-media-config.mjs';
import { validateArchiveEntries, verifyArchive } from './native-media-source.mjs';
import { validateDependency, verifyInstall, writeManifest } from './native-media-manifest.mjs';
import { windowsToolchain } from './native-media-windows.mjs';

test('unsupported targets fail instead of compiling with the host toolchain', () => {
  for (const target of ['', 'aarch64-unknown-linux-gnu', 'macos']) {
    assert.throws(() => targetSpec(target), /Unsupported native media target/);
  }
});

test('desktop and mobile configurations preserve decoder, parser and tempo capabilities', () => {
  for (const target of [
    'aarch64-apple-darwin',
    'x86_64-pc-windows-msvc',
    'aarch64-apple-ios',
    'aarch64-linux-android',
  ]) {
    const args = configuration(targetSpec(target), '/prefix', { args: ['--cc=target-clang'] });
    assert.ok(args.includes('--disable-autodetect'));
    assert.ok(args.includes('--disable-mediafoundation'));
    assert.ok(args.includes('--disable-avformat'));
    assert.ok(args.includes('--disable-programs'));
    assert.ok(args.includes('--disable-encoders'));
    assert.ok(args.includes('--disable-gpl'));
    assert.ok(args.includes('--disable-nonfree'));
    assert.ok(args.includes('--enable-decoder=h264,hevc,aac'));
    assert.ok(args.includes('--enable-parser=h264,hevc,aac'));
    assert.ok(args.includes('--enable-filter=abuffer,abuffersink,atempo,aformat,aresample,anull'));
    assert.ok(
      args.includes(
        target.includes('darwin') || target.includes('windows')
          ? '--enable-shared'
          : '--enable-static',
      ),
    );
  }
});

test('Windows cross builds require explicit target SDK and compiler inputs', () => {
  const unexpectedExecution = () => assert.fail('compiler must not run without a target SDK');
  assert.throws(() => windowsToolchain(unexpectedExecution, {}, 'darwin'), /Set XWIN_CACHE_DIR/);
  assert.throws(
    () => windowsToolchain(unexpectedExecution, {}, 'linux'),
    /requires Windows MSVC or macOS xwin/,
  );
});

test('Windows hardware configurations include the modern D3D11 texture output', () => {
  const toolchain = windowsToolchain(() => 'MSVC fixture', {}, 'win32');
  const args = configuration(targetSpec('x86_64-pc-windows-msvc'), '/prefix', toolchain);
  const accelerators = args.find((argument) => argument.startsWith('--enable-hwaccel='));
  assert.ok(accelerators.includes('h264_d3d11va2'));
  assert.ok(accelerators.includes('hevc_d3d11va2'));
  assert.ok(accelerators.includes('h264_d3d12va'));
  assert.ok(accelerators.includes('hevc_d3d12va'));
  assert.ok(args.includes('--cpu=generic'));
  assert.ok(args.includes('--extra-cflags=-MT -D_WIN32_WINNT=0x0A00'));
});

test('Windows Android builds invoke the NDK executable with an explicit API target', (context) => {
  const ndk = mkdtempSync(join(tmpdir(), 'hongguo-ndk-'));
  context.after(() => rmSync(ndk, { recursive: true, force: true }));
  const bin = join(ndk, 'toolchains/llvm/prebuilt/windows-x86_64/bin');
  mkdirSync(bin, { recursive: true });
  const compiler = join(bin, 'clang.exe').replaceAll('\\', '/');
  writeFileSync(compiler, 'fixture');
  const toolchain = resolveToolchain(
    targetSpec('aarch64-linux-android'),
    (command, args) => {
      assert.equal(command, compiler);
      assert.deepEqual(args, ['--target=aarch64-linux-android28', '--version']);
      return 'NDK clang fixture';
    },
    { NDK_HOME: ndk },
    'win32',
  );
  assert.equal(toolchain.compiler, compiler);
  assert.ok(toolchain.args.includes('--extra-cflags=--target=aarch64-linux-android28'));
  assert.ok(toolchain.args.includes('--extra-ldflags=--target=aarch64-linux-android28'));
});

test('source archives cannot escape their versioned root', () => {
  assert.doesNotThrow(() => validateArchiveEntries('ffmpeg-9.0.2/\nffmpeg-9.0.2/configure\n'));
  for (const listing of [
    '',
    '/tmp/configure\n',
    'ffmpeg-9.0.2/../configure\n',
    'other/configure\n',
  ]) {
    assert.throws(() => validateArchiveEntries(listing), /Invalid FFmpeg archive/);
  }
});

test('source digest is verified before archive extraction', () => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-native-source-'));
  try {
    const file = join(directory, 'source.tar.xz');
    const bytes = Buffer.from('verified release');
    writeFileSync(file, bytes);
    const digest = createHash('sha256').update(bytes).digest('hex');
    assert.equal(verifyArchive(file, digest), digest);
    writeFileSync(file, 'tampered release');
    assert.throws(() => verifyArchive(file, digest), /FFmpeg source SHA-256 mismatch/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('runtime dependency closure accepts only bundled media or operating system libraries', () => {
  const macos = targetSpec('aarch64-apple-darwin');
  for (const dependency of [
    '/usr/lib/libSystem.B.dylib',
    '/System/Library/Frameworks/CoreVideo.framework/Versions/A/CoreVideo',
    '@rpath/libavutil.61.dylib',
  ]) {
    assert.doesNotThrow(() => validateDependency(macos, dependency));
  }
  for (const dependency of [
    '/opt/homebrew/lib/libavutil.dylib',
    '/tmp/libavcodec.dylib',
    '@rpath/libx264.dylib',
  ]) {
    assert.throws(() => validateDependency(macos, dependency), /Unexpected runtime dependency/);
  }
  const windows = targetSpec('x86_64-pc-windows-msvc');
  assert.doesNotThrow(() => validateDependency(windows, 'avutil-61.dll'));
  assert.doesNotThrow(() => validateDependency(windows, 'KERNEL32.dll'));
  for (const dependency of [
    'VCRUNTIME140.dll',
    'libgcc_s_seh-1.dll',
    'mfplat.dll',
    'unbundled.dll',
  ]) {
    assert.throws(() => validateDependency(windows, dependency), /Unexpected runtime dependency/);
  }
});

test('a built prefix cannot pass verification after artifact tampering or target substitution', () => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-native-install-'));
  const spec = targetSpec('aarch64-apple-darwin');
  try {
    mkdirSync(join(directory, 'lib'));
    const library = join(directory, 'lib/libavcodec.dylib');
    writeFileSync(library, 'built library');
    writeManifest(directory, { source, target: spec });
    assert.equal(verifyInstall(directory, spec).artifacts.length, 1);
    assert.throws(
      () => verifyInstall(directory, targetSpec('x86_64-apple-darwin')),
      /does not match/,
    );
    writeFileSync(library, 'broken library');
    assert.throws(() => verifyInstall(directory, spec), /missing or modified/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
