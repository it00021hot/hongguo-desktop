import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

function invoke(context, adapter, args, exitCode = 0) {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-windows-adapter-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const compiler = join(directory, 'compiler.cjs');
  writeFileSync(
    compiler,
    'process.stdout.write(JSON.stringify(process.argv.slice(2))); process.exitCode = Number(process.env.NSH_TEST_COMPILER_EXIT);',
  );
  const launcher = adapter === 'clang';
  return spawnSync(
    process.execPath,
    [
      fileURLToPath(new URL(`./nsh-windows-${adapter}.cjs`, import.meta.url)),
      ...(launcher ? ['cmake-compiler', compiler] : [compiler]),
      ...args,
    ],
    {
      encoding: 'utf8',
      env: {
        ...process.env,
        [launcher ? 'NSH_WINDOWS_CLANG' : 'NSH_WINDOWS_RC']: process.execPath,
        NSH_TEST_COMPILER_EXIT: String(exitCode),
      },
    },
  );
}

test('clang keeps MSVC warning levels in the driver and frontend warning overrides in order', (context) => {
  const result = invoke(context, 'clang', [
    '-W0',
    '-Wall',
    '-Wno-unused-function',
    '-Werror',
    '-Wno-unused-command-line-argument',
    '--',
    'source with spaces.cpp',
  ]);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), [
    '-W0',
    '/clang:-Wall',
    '/clang:-Wno-unused-function',
    '-Werror',
    '-Wno-unused-command-line-argument',
    '--',
    'source with spaces.cpp',
  ]);
});

test('resource output paths support both joined and separate Windows options', (context) => {
  for (const output of [['/fooutput with spaces.res'], ['/fo', 'output with spaces.res']]) {
    const result = invoke(context, 'rc', ['/nologo', ...output, 'input with spaces.rc']);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), [
      '-nologo',
      '-fo',
      'output with spaces.res',
      '--',
      'input with spaces.rc',
    ]);
  }
});

test('a failing compiler remains a failed build through either adapter', (context) => {
  const failureExitCode = 7;
  for (const adapter of ['clang', 'rc']) {
    const result = invoke(context, adapter, ['input'], failureExitCode);
    assert.equal(result.status, failureExitCode);
  }
});
