import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { replayLink, responseArguments } from './native-media-relink.mjs';
import { finishRelink } from './nsh-relink.mjs';

test('linker response files preserve quoted paths and reject incomplete quoting', () => {
  assert.deepEqual(responseArguments('"file with spaces.o" -L"a b" \'literal\\path\' ""'), [
    'file with spaces.o',
    '-La b',
    'literal\\path',
    '',
  ]);
  for (const input of ['"unfinished', 'unfinished\\'])
    assert.throws(() => responseArguments(input), /quoting/);
});

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

test(
  'large quoted links preserve replaceable real objects after original inputs are removed',
  { skip: process.platform !== 'darwin' },
  (context) => {
    const directory = mkdtempSync(join(tmpdir(), 'hongguo relink-'));
    context.after(() => rmSync(directory, { recursive: true, force: true }));
    const compiler = run('xcrun', ['--find', 'clang']);
    const sdk = run('xcrun', ['--show-sdk-path']);
    const original = join(directory, 'original');
    const replacement = join(directory, 'replacement');
    const materials = join(directory, 'Relink');
    const buildLibrary = (prefix, value) => {
      mkdirSync(join(prefix, 'lib'), { recursive: true });
      const source = join(prefix, 'value.c');
      const object = join(prefix, 'value.o');
      writeFileSync(source, `int fixture_value(void) { return ${value}; }\n`);
      run(compiler, ['-isysroot', sdk, '-c', source, '-o', object]);
      for (const name of ['de265']) run('ar', ['rcs', join(prefix, 'lib', `lib${name}.a`), object]);
    };
    buildLibrary(original, 7);
    buildLibrary(replacement, 9);
    const main = join(directory, 'main.c');
    const object = join(directory, 'main.o');
    writeFileSync(main, 'int fixture_value(void); int main(void) { return fixture_value(); }\n');
    run(compiler, ['-isysroot', sdk, '-c', main, '-o', object]);
    const output = join(directory, 'before');
    const response = join(original, 'link.args');
    writeFileSync(
      response,
      [
        object,
        '-isysroot',
        sdk,
        '-L',
        join(original, 'lib'),
        '-lde265',
        ...Array.from({ length: 1000 }, () => '-Wl,-dead_strip'),
        '-o',
        output,
      ]
        .map((value) => JSON.stringify(value))
        .join('\n'),
    );
    run(
      process.execPath,
      [fileURLToPath(new URL('./native-media-link.cjs', import.meta.url)), `@${response}`],
      {
        env: {
          ...process.env,
          HONGGUO_RELINK_REAL_LINKER: compiler,
          HONGGUO_RELINK_OUTPUT: materials,
          HONGGUO_RELINK_SDK_ROOT: sdk,
          HONGGUO_HEVC_ROOT: original,
          HONGGUO_RELINK_TARGET: 'fixture-host',
        },
      },
    );
    assert.equal(spawnSync(output).status, 7);
    run('strip', [output]);
    finishRelink(output, 'fixture-host', materials);
    const stripped = JSON.parse(readFileSync(join(materials, 'link.json'), 'utf8'));
    assert.notEqual(stripped.linkerOutputSha256, stripped.outputSha256);
    rmSync(original, { recursive: true });
    rmSync(main);
    rmSync(object);
    const after = join(directory, 'after');
    replayLink(materials, { linker: compiler, sdk, prefix: replacement, output: after });
    assert.equal(spawnSync(after).status, 9);
    const manifest = JSON.parse(readFileSync(join(materials, 'link.json'), 'utf8'));
    assert.ok(manifest.files.some((file) => file.path.startsWith('objects/')));
    writeFileSync(join(materials, manifest.files[0].path), 'changed material');
    assert.throws(
      () => replayLink(materials, { linker: compiler, sdk, prefix: replacement, output: after }),
      /missing or modified/,
    );
  },
);
