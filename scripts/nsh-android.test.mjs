import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

test('Android packaging requires the matching embedding SDK before Android tools or Cargo', () => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-nsh-android-'));
  try {
    const env = { ...process.env, NSH_DISTRIBUTION: directory, PATH: directory };
    delete env.ANDROID_HOME;
    delete env.NDK_HOME;
    const result = spawnSync(
      process.execPath,
      [fileURLToPath(new URL('./nsh.mjs', import.meta.url)), 'package', 'android'],
      { encoding: 'utf8', env },
    );
    assert.ifError(result.error);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /NSH 0\.5\.0 Android embedding SDK/);
    assert.match(result.stderr, /nsh-embed-android\.zip/);
    assert.ok(result.stderr.includes(join(directory, 'embed/android/nsh.aar')));
    assert.doesNotMatch(result.stderr, /ANDROID_HOME|NDK_HOME|NDK LLVM|cargo/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
