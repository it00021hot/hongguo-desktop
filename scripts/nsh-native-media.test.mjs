import assert from 'node:assert/strict';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { prepareNativeSdk } from './native-sdk.mjs';
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

test('desktop delivery includes one codec DLL and authenticated corresponding source', (context) => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-media-package-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const triple = 'x86_64-pc-windows-msvc';
  const prefix = prepareNativeSdk(triple, { destination: join(directory, 'sdk') });
  const destination = join(directory, 'Hongguo');
  mkdirSync(join(destination, 'NativeMedia/ffmpeg'), { recursive: true });
  writeFileSync(join(destination, 'NativeMedia/ffmpeg/obsolete.dll'), 'old runtime');
  copyDesktopMedia(prefix, triple, destination);
  assert.equal(existsSync(join(destination, 'NativeMedia/ffmpeg')), false);
  assert.deepEqual(
    readFileSync(join(destination, 'libde265.dll')),
    readFileSync(join(prefix, 'bin/libde265.dll')),
  );
  const manifest = JSON.parse(
    readFileSync(join(destination, 'NativeMedia/libde265/source-manifest.json'), 'utf8'),
  );
  assert.equal(manifest.source.version, '1.1.3');
  assert.ok(readFileSync(join(destination, 'NativeMedia/libde265/licenses/COPYING')).length);
  assert.ok(readFileSync(join(destination, 'NativeMedia/vvdec/source/LICENSE.txt')).length);
  assert.ok(
    readFileSync(join(destination, 'NativeMedia/build-scripts/native-codec-cmake/CMakeLists.txt'))
      .length,
  );
  writeFileSync(join(destination, 'Hongguo.exe'), 'fixture');
  writeFileSync(join(destination, 'WebView2Loader.dll'), 'unapproved');
  assert.throws(() => auditWindowsBundle(destination), /exactly the pinned libde265 DLL/);
});

test(
  'macOS delivery keeps relative dylib symlinks valid inside Frameworks',
  { skip: process.platform === 'win32' },
  (context) => {
    const directory = mkdtempSync(join(tmpdir(), 'hongguo-media-symlinks-'));
    context.after(() => rmSync(directory, { recursive: true, force: true }));
    const triple = 'aarch64-apple-darwin';
    const prefix = prepareNativeSdk(triple, { destination: join(directory, 'sdk') });
    const destination = join(directory, 'Hongguo.app');
    copyDesktopMedia(prefix, triple, destination);
    const link = join(destination, 'Contents/Frameworks/libde265.dylib');
    assert.equal(readlinkSync(link), 'libde265.0.dylib');
    assert.deepEqual(readFileSync(link), readFileSync(join(prefix, 'lib/libde265.dylib')));
  },
);
