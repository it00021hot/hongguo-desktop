import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

assert.equal(process.platform, 'darwin', 'This native playback regression requires macOS');
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const output = join(root, 'target/nsh-visibility-playback');
mkdirSync(output, { recursive: true });

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8' });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  return result;
}

run('swiftc', ['scripts/nsh-visibility-cover.swift', '-o', 'target/nsh-visibility-cover']);
run('cargo', ['build', '-p', 'hongguo-nsh', '--example', 'visibility_playback']);
const media = pathToFileURL(
  join(root, 'crates/hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4'),
).href;
const composite = ['--nonopaque', '--translucent'];
const scenarios = [
  ['opaque', [], true],
  ['composite', composite, true],
  ['partial', [...composite, '--partial'], false],
  ['manual-pause', [...composite, '--paused'], true],
  ['disabled', [...composite, '--disabled'], false],
];

function samples(directory, name) {
  return readFileSync(join(directory, `${name}.jsonl`), 'utf8')
    .trim()
    .split('\n')
    .map((line) => JSON.parse(line));
}

for (const [name, args, pauses] of scenarios) {
  const directory = mkdtempSync(join(output, `${name}-`));
  const result = run(join(root, 'target/debug/examples/visibility_playback'), [
    media,
    directory,
    ...args,
  ]);
  writeFileSync(join(directory, 'output.log'), result.stderr);
  assert.doesNotMatch(result.stderr, /panicked|ERROR/);
  const native = samples(directory, 'native');
  const policy = samples(directory, 'policy');
  const covered = native.filter((s) => s.elapsed > 1 && s.elapsed < 1.5);
  assert.ok(covered.length >= 3, 'enough independent native rate observations');
  assert.ok(
    covered.every((s) => s.nativeRate === (pauses ? 0 : 1)),
    `${name}: native pause state`,
  );
  if (name === 'composite') {
    assert.ok(
      covered.every((s) => s.nativeExposed),
      'reproduce AppKit reporting visible beneath composite cover',
    );
  }
  if (name === 'manual-pause') {
    assert.ok(
      native.filter((s) => s.nativeRate !== null).every((s) => s.nativeRate === 0),
      'never resume a manually paused video',
    );
  } else if (pauses) {
    assert.ok(
      native.some((s) => s.elapsed > 3.8 && s.elapsed < 4.6 && s.nativeRate === 1),
      'resume after uncovering',
    );
    const positions = policy.filter((s) => s.elapsed > 1 && s.elapsed < 2.8).map((s) => s.position);
    assert.ok(positions.length >= 5);
    assert.ok(
      Math.max(...positions) - Math.min(...positions) < 0.05,
      'native timeline remains stopped while covered',
    );
  }
  assert.ok(
    policy.some((s) => s.elapsed > 1 && s.elapsed < 2 && s.allowed === !pauses),
    `${name}: playback admission`,
  );
  process.stdout.write(`PASS ${name}: ${directory}\n`);
}
