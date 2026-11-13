import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { test } from 'node:test';
import { buildDesktop, desktopExecutable, validateMacosPlatform } from './nsh-desktop.mjs';

test('macOS packaging rejects iOS, simulators, Catalyst, and missing platform records', () => {
  const record = (platform) =>
    `Load command 11\n      cmd LC_BUILD_VERSION\n  cmdsize 32\n platform ${platform}\n    minos 14.0\n`;
  assert.doesNotThrow(() => validateMacosPlatform(record(1)));
  for (const commands of ['', record(2), record(6), record(7), record(1) + record(7)]) {
    assert.throws(() => validateMacosPlatform(commands), /macOS platform/);
  }
});

function artifact(executable, name = 'hongguo-nsh') {
  return JSON.stringify({
    reason: 'compiler-artifact',
    target: { name, kind: ['bin'] },
    executable,
  });
}

test('packaging uses the executable Cargo reports in a custom target layout', () => {
  const executable = join(
    tmpdir(),
    'custom target',
    'aarch64-apple-darwin',
    'release',
    'hongguo-nsh',
  );
  const output = [
    artifact(join(tmpdir(), 'other-tool'), 'other-tool'),
    artifact(null),
    artifact(executable),
    JSON.stringify({ reason: 'build-finished', success: true }),
  ].join('\n');
  assert.equal(desktopExecutable(output), executable);
});

test('missing, ambiguous, or relative executables cannot fall back to an old binary', () => {
  const executable = join(tmpdir(), 'hongguo-nsh');
  for (const output of [
    '',
    artifact(null),
    artifact('release/hongguo-nsh'),
    [artifact(executable), artifact(executable)].join('\n'),
  ]) {
    assert.throws(() => desktopExecutable(output), /exactly one desktop executable/);
  }
  assert.throws(() => desktopExecutable('invalid Cargo output'), SyntaxError);
});

test('desktop packaging refuses a different host before building', () => {
  const target = process.platform === 'darwin' ? 'windows' : 'macos';
  assert.throws(() => buildDesktop(target), /Desktop packaging requires/);
});

test(
  'QuickJS in production dependencies stops packaging before Cargo build',
  {
    skip: process.platform !== 'darwin',
  },
  () => {
    const directory = mkdtempSync(join(tmpdir(), 'hongguo-nsh-desktop-'));
    try {
      writeFileSync(
        join(directory, 'cargo'),
        '#!/bin/sh\nif [ "$1" = "tree" ]; then\n  printf "%s\\n" "nsh-js v0.4.0" "quickjs-jit v0.14.1"\nelse\n  exit 91\nfi\n',
        { mode: 0o755 },
      );
      const module = new URL('./nsh-desktop.mjs', import.meta.url).href;
      const result = spawnSync(
        process.execPath,
        [
          '--input-type=module',
          '-e',
          `import { buildDesktop } from ${JSON.stringify(module)}; buildDesktop('macos');`,
        ],
        {
          encoding: 'utf8',
          env: { ...process.env, PATH: `${directory}${delimiter}${process.env.PATH ?? ''}` },
        },
      );
      assert.ifError(result.error);
      assert.equal(result.status, 1);
      assert.match(result.stderr, /pure Rust desktop host must not include QuickJS/);
      assert.doesNotMatch(result.stderr, /exited with 91/);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
);
