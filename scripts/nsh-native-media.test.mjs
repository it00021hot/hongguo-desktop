import assert from 'node:assert/strict';
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { source, targetSpec } from './native-media-config.mjs';
import { writeManifest } from './native-media-manifest.mjs';
import {
  auditWindowsBundle,
  copyDesktopMedia,
  desktopTriple,
  validateNshSource,
} from './nsh-native-media.mjs';

test('desktop targets preserve the host architecture and reject unsupported combinations', () => {
  assert.equal(desktopTriple('macos', 'arm64'), 'aarch64-apple-darwin');
  assert.equal(desktopTriple('macos', 'x64'), 'x86_64-apple-darwin');
  assert.equal(desktopTriple('windows', 'x64'), 'x86_64-pc-windows-msvc');
  assert.throws(() => desktopTriple('windows', 'arm64'), /Unsupported desktop media target/);
});

test('CLI and production builds cannot resolve a second NSH core or testing source', () => {
  const path = fileURLToPath(new URL('../vendor/nsh/crates/nsh-core/Cargo.toml', import.meta.url));
  const entry = { name: 'nsh-core', source: null, manifest_path: path };
  assert.doesNotThrow(() => validateNshSource({ packages: [entry] }));
  for (const foreign of [
    { ...entry, source: 'git+https://github.com/nativeshell/nsh' },
    { ...entry, name: 'nsh-testing', source: 'registry+crates.io' },
    { ...entry, manifest_path: fileURLToPath(new URL('../Cargo.toml', import.meta.url)) },
  ])
    assert.throws(() => validateNshSource({ packages: [entry, foreign] }), /vendored source/);
});

function fixture(directory, triple) {
  const prefix = join(directory, 'prefix');
  for (const name of ['licenses', 'sources', 'build-info']) {
    mkdirSync(join(prefix, name), { recursive: true });
    writeFileSync(join(prefix, name, 'fixture.txt'), name);
  }
  return { prefix, target: targetSpec(triple) };
}

test('desktop delivery includes all four DLLs and exact corresponding source materials', (context) => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-media-package-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const triple = 'x86_64-pc-windows-msvc';
  const { prefix, target } = fixture(directory, triple);
  mkdirSync(join(prefix, 'bin'));
  const names = ['avcodec-63.dll', 'avutil-61.dll', 'swresample-7.dll', 'avfilter-12.dll'];
  for (const name of names) writeFileSync(join(prefix, 'bin', name), `fixture ${name}`);
  writeManifest(prefix, { source, target });
  const destination = join(directory, 'Hongguo');
  copyDesktopMedia(prefix, triple, destination);
  for (const name of names)
    assert.equal(readFileSync(join(destination, name), 'utf8'), `fixture ${name}`);
  assert.equal(
    readFileSync(join(destination, 'NativeMedia/ffmpeg/sources/fixture.txt'), 'utf8'),
    'sources',
  );
  const manifest = JSON.parse(
    readFileSync(join(destination, 'NativeMedia/ffmpeg/source-manifest.json'), 'utf8'),
  );
  assert.equal(manifest.source.sha256, source.sha256);
  writeFileSync(join(destination, 'Hongguo.exe'), 'fixture');
  writeFileSync(join(destination, 'WebView2Loader.dll'), 'unapproved');
  assert.throws(() => auditWindowsBundle(destination), /exactly the four pinned FFmpeg DLLs/);
});

test('macOS delivery keeps relative dylib symlinks valid inside Frameworks', (context) => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-media-symlinks-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const triple = 'aarch64-apple-darwin';
  const { prefix, target } = fixture(directory, triple);
  mkdirSync(join(prefix, 'lib'));
  writeFileSync(join(prefix, 'lib/libavutil.61.dylib'), 'fixture dylib');
  symlinkSync('libavutil.61.dylib', join(prefix, 'lib/libavutil.dylib'));
  writeManifest(prefix, { source, target });
  const destination = join(directory, 'Hongguo.app');
  copyDesktopMedia(prefix, triple, destination);
  const link = join(destination, 'Contents/Frameworks/libavutil.dylib');
  assert.equal(readlinkSync(link), 'libavutil.61.dylib');
  assert.equal(readFileSync(link, 'utf8'), 'fixture dylib');
});
