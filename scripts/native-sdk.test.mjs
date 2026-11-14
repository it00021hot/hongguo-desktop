import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import {
  prepareNativeSdk,
  runtimeEnvironment,
  rustTarget,
  verifyNativeSdk,
} from './native-sdk.mjs';

const triple = 'aarch64-apple-darwin';
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');

function fixture(context) {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-sdk-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const root = join(directory, 'source', triple);
  const paths = [
    'libde265-1.1.3/manifest.json',
    'vvdec-3.2.1/manifest.json',
    'vvdec-3.2.1/bindings.rs',
  ];
  const artifacts = paths.map((path) => {
    const bytes = Buffer.from(path);
    mkdirSync(join(root, path, '..'), { recursive: true });
    writeFileSync(join(root, path), bytes);
    return { path, sha256: hash(bytes), byteLength: bytes.length };
  });
  const manifest = {
    schemaVersion: 1,
    sdkVersion: 'test-v1',
    target: { triple },
    recipeSha256: 'fixture-recipe',
    components: {
      hevc: { manifestPath: paths[0] },
      vvdec: { manifestPath: paths[1] },
    },
    artifacts,
  };
  const bytes = Buffer.from(`${JSON.stringify(manifest)}\n`);
  writeFileSync(join(root, 'sdk.json'), bytes);
  const archive = join(directory, 'fixture.tar.gz');
  const result = spawnSync('tar', ['-czf', archive, '-C', join(directory, 'source'), triple]);
  assert.equal(result.status, 0, result.stderr?.toString());
  const data = readFileSync(archive);
  const lock = {
    schemaVersion: 1,
    sdkVersion: 'test-v1',
    releaseTag: 'fixture-v1',
    targets: {
      [triple]: {
        assetName: 'fixture.tar.gz',
        url: 'https://example.invalid/fixture.tar.gz',
        sha256: hash(data),
        byteLength: data.length,
        manifestSha256: hash(bytes),
        recipeSha256: 'fixture-recipe',
      },
    },
  };
  return { directory, root, archive, lock, manifest };
}

test('a clean startup installs the authenticated SDK once and reuses verified cache', (context) => {
  const data = fixture(context);
  const destination = join(data.directory, 'installed', triple);
  let downloads = 0;
  const options = {
    lock: data.lock,
    destination,
    download: (_url, path) => {
      downloads++;
      cpSync(data.archive, path);
    },
  };
  const prefix = prepareNativeSdk(triple, options);
  assert.equal(prefix, join(destination, 'libde265-1.1.3'));
  assert.equal(downloads, 1);
  prepareNativeSdk(triple, options);
  assert.equal(downloads, 1);
  assert.equal(
    readFileSync(join(destination, 'vvdec-3.2.1/bindings.rs'), 'utf8'),
    'vvdec-3.2.1/bindings.rs',
  );
});

test('changed archives and SDK manifests cannot become a trusted cache', (context) => {
  const data = fixture(context);
  const destination = join(data.directory, 'installed', triple);
  const options = {
    lock: data.lock,
    destination,
    download: (_url, path) => writeFileSync(path, 'tampered'),
  };
  assert.throws(() => prepareNativeSdk(triple, options), /SHA-256|size/i);
  assert.equal(existsSync(join(destination, 'sdk.json')), false);
  writeFileSync(join(data.root, 'sdk.json'), '{}');
  assert.throws(() => verifyNativeSdk(triple, data.root, data.lock), /manifest.*SHA-256/i);
});

test('modified bindings are rejected even when the cached manifest is unchanged', (context) => {
  const data = fixture(context);
  writeFileSync(join(data.root, 'vvdec-3.2.1/bindings.rs'), 'modified');
  assert.throws(() => verifyNativeSdk(triple, data.root, data.lock), /artifact|SHA-256|size/i);
});

test('target mismatches and artifact escapes are rejected before consumption', (context) => {
  const data = fixture(context);
  assert.throws(() => verifyNativeSdk('x86_64-apple-darwin', data.root, data.lock), /target/i);
  data.manifest.artifacts[0].path = '../outside';
  const bytes = Buffer.from(JSON.stringify(data.manifest));
  writeFileSync(join(data.root, 'sdk.json'), bytes);
  data.lock.targets[triple].manifestSha256 = hash(bytes);
  assert.throws(() => verifyNativeSdk(triple, data.root, data.lock), /path|escape/i);
});

test('SDK installation leaves unrelated source-build cache files intact', (context) => {
  const data = fixture(context);
  const destination = join(data.directory, 'installed', triple);
  mkdirSync(destination, { recursive: true });
  writeFileSync(join(destination, 'build.log'), 'keep source-build diagnostics');
  prepareNativeSdk(triple, {
    lock: data.lock,
    destination,
    download: (_url, path) => cpSync(data.archive, path),
  });
  assert.equal(
    readFileSync(join(destination, 'build.log'), 'utf8'),
    'keep source-build diagnostics',
  );
});

test('a corrupted cache is replaced from the authenticated archive', (context) => {
  const data = fixture(context);
  const destination = join(data.directory, 'installed', triple);
  const options = {
    lock: data.lock,
    destination,
    download: (_url, path) => cpSync(data.archive, path),
  };
  prepareNativeSdk(triple, options);
  writeFileSync(join(destination, 'vvdec-3.2.1/bindings.rs'), 'broken');
  prepareNativeSdk(triple, options);
  assert.doesNotThrow(() => verifyNativeSdk(triple, destination, data.lock));
});

test(
  'a symlink outside the SDK cannot supply an authenticated artifact',
  { skip: process.platform === 'win32' },
  (context) => {
    const data = fixture(context);
    const path = join(data.root, 'vvdec-3.2.1/bindings.rs');
    const outside = join(data.directory, 'outside.rs');
    writeFileSync(outside, readFileSync(path));
    rmSync(path);
    symlinkSync(outside, path);
    assert.throws(() => verifyNativeSdk(triple, data.root, data.lock), /escapes/);
  },
);

test('an authenticated archive for a different top-level target is rejected', (context) => {
  const data = fixture(context);
  const root = join(data.directory, 'wrong');
  mkdirSync(root);
  writeFileSync(join(root, 'unexpected'), 'not a target directory');
  const result = spawnSync('tar', ['-czf', data.archive, '-C', root, 'unexpected']);
  assert.equal(result.status, 0);
  const archive = readFileSync(data.archive);
  Object.assign(data.lock.targets[triple], { sha256: hash(archive), byteLength: archive.length });
  assert.throws(
    () =>
      prepareNativeSdk(triple, {
        lock: data.lock,
        destination: join(data.directory, 'installed', triple),
        download: (_url, path) => cpSync(data.archive, path),
      }),
    /unexpected target path/,
  );
});

test('runtime environment adds private libraries without requiring a global install', () => {
  const windows = runtimeEnvironment('D:\\sdk\\libde265-1.1.3', {
    platform: 'win32',
    env: { PATH: 'D:\\Rust' },
  });
  assert.equal(windows.PATH, 'D:\\sdk\\libde265-1.1.3\\bin;D:\\Rust');
  const mac = runtimeEnvironment('/sdk/libde265-1.1.3', {
    platform: 'darwin',
    env: { DYLD_LIBRARY_PATH: '/previous' },
  });
  assert.equal(mac.DYLD_LIBRARY_PATH, '/sdk/libde265-1.1.3/lib:/previous');
});

test('target resolution follows Cargo target selection and otherwise Rust host', () => {
  const run = () => 'rustc 1.99.1\nhost: x86_64-pc-windows-msvc\n';
  assert.equal(rustTarget({}, run), 'x86_64-pc-windows-msvc');
  assert.equal(
    rustTarget({ CARGO_BUILD_TARGET: triple }, () => {
      throw new Error('unused');
    }),
    triple,
  );
  assert.throws(() => rustTarget({}, () => 'rustc without host'), /host/i);
});
