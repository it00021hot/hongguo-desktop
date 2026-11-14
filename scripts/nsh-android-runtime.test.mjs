import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { buildAndroidRuntime } from './nsh-android-runtime.mjs';

function run(command, args, env = {}, cwd) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    cwd,
    env: { ...process.env, ...env },
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}

test('missing Android compile SDK fails before running the Java toolchain', () => {
  assert.throws(
    () => buildAndroidRuntime({ sdk: '/missing-hongguo-sdk' }, () => assert.fail('must not run')),
    /Android SDK 37/,
  );
});

test(
  'runtime AAR uses current Java sources and omits the SDK native libraries',
  {
    skip:
      spawnSync('javac', ['-version']).status !== 0 || spawnSync('jar', ['--version']).status !== 0,
  },
  (context) => {
    const directory = mkdtempSync(join(tmpdir(), 'hongguo runtime-'));
    context.after(() => rmSync(directory, { recursive: true, force: true }));
    const payload = join(directory, 'sdk-payload');
    const platform = join(directory, 'sdk/platforms/android-37.0');
    const empty = join(directory, 'empty');
    const source = join(directory, 'source');
    for (const path of [payload, platform, empty, source]) mkdirSync(path, { recursive: true });
    run('jar', ['cf', join(platform, 'android.jar'), '-C', empty, '.']);
    writeFileSync(
      join(platform, 'source.properties'),
      'AndroidVersion.ApiLevel=37.0\r\nAndroidVersion.CodeName=\r\nPkg.Revision=2\r\n',
    );
    run('jar', ['cf', join(payload, 'classes.jar'), '-C', empty, '.']);
    for (const file of ['AndroidManifest.xml', 'R.txt', 'proguard.txt'])
      writeFileSync(join(payload, file), file);
    mkdirSync(join(payload, 'jni/arm64-v8a'), { recursive: true });
    writeFileSync(join(payload, 'jni/arm64-v8a/libnsh.so'), 'SDK binary must not be packaged');
    const aar = join(directory, 'sdk.aar');
    run('jar', ['cf', aar, '-C', payload, '.']);
    const consumer = join(directory, 'Consumer.java');
    writeFileSync(
      consumer,
      'public class Consumer { public static void main(String[] args) { System.exit(Current.value()); } }',
    );
    const before = spawnSync(
      'javac',
      ['-classpath', join(payload, 'classes.jar'), '-d', empty, consumer],
      { encoding: 'utf8' },
    );
    assert.equal(before.status, 1);
    assert.match(before.stderr, /Current/);
    writeFileSync(
      join(source, 'Current.java'),
      'public class Current { public static int value() { return 2; } }',
    );
    const output = join(directory, 'output');
    const runtime = buildAndroidRuntime(
      { aar, sdk: join(directory, 'sdk'), output, sourceDirectory: source },
      run,
    );
    const entries = run('jar', ['tf', runtime]);
    assert.doesNotMatch(entries, /jni\/|libnsh\.so/);
    const extracted = join(directory, 'extracted');
    mkdirSync(extracted);
    run('jar', ['xf', runtime, 'classes.jar'], {}, extracted);
    const classes = join(extracted, 'classes.jar');
    run('jar', ['xf', classes, 'Current.class'], {}, extracted);
    assert.equal(readFileSync(join(extracted, 'Current.class')).readUInt16BE(6), 61);
    run('javac', ['-classpath', classes, '-d', empty, consumer]);
    assert.equal(
      spawnSync('java', [
        '-classpath',
        [empty, classes].join(process.platform === 'win32' ? ';' : ':'),
        'Consumer',
      ]).status,
      2,
    );
    const manifest = JSON.parse(readFileSync(join(output, 'runtime-source.json'), 'utf8'));
    assert.equal(manifest.sources.length, 1);
    assert.equal(manifest.sources[0].path, 'Current.java');
    assert.equal(manifest.runtimeSha256.length, 64);
  },
);
