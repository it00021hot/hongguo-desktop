import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { access, mkdir, mkdtemp, rename, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { test } from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const enabled = process.platform === 'darwin' && process.env.HONGGUO_NSH_MACOS_PIXEL_TEST === '1';
const commandTimeout = 30_000;
const paintTimeout = 30_000;
const pollInterval = 500;
const shutdownTimeout = 5_000;
const scenarioComplete = 'PIXEL_SCENARIO_COMPLETE';

test(
  'macOS native window displays pixels and follows animated sidebar resizing',
  { skip: !enabled, timeout: 150_000 },
  async (context) => {
    const binary = join(root, 'target/debug/examples/pixel_surface');
    await access(binary);
    await mkdir(join(root, 'target/nsh-macos-pixel'), { recursive: true });
    const directory = await mkdtemp(join(root, 'target/nsh-macos-pixel/run-'));
    context.diagnostic(`Pixel screenshots and logs: ${directory}`);
    const helper = join(directory, 'pixel-inspect');
    run('swiftc', [join(root, 'scripts/nsh-macos-pixel.swift'), '-o', helper]);
    const audio = join(directory, 'audio.m4a');
    run('ffmpeg', [
      '-nostdin',
      '-v',
      'error',
      '-i',
      join(root, 'crates/hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4'),
      '-vn',
      '-c:a',
      'copy',
      audio,
    ]);
    for (const mode of ['checker', 'hevc']) {
      await verifyWindow({ binary, helper, audio, directory, mode });
    }
    await verifyWindow({ binary, helper, audio, directory, mode: 'checker', resize: true });
  },
);

function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8', timeout: commandTimeout });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${command} exited ${result.status}: ${result.stderr}`);
  return result.stdout;
}

async function verifyWindow({ binary, helper, audio, directory, mode, resize = false }) {
  const control = resize ? join(directory, 'resize-control') : undefined;
  if (control) await writeFile(control, 'expanded');
  const child = spawn(binary, [pathToFileURL(audio).href, mode, ...(control ? [control] : [])], {
    cwd: root,
    env: { ...process.env, RUST_LOG: 'info' },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const exited = new Promise((resolve) => child.once('exit', resolve));
  let output = '';
  child.stdout.on('data', (chunk) => {
    output += chunk;
  });
  child.stderr.on('data', (chunk) => {
    output += chunk;
  });
  await once(child, 'spawn');
  const screenshot = join(directory, `${mode}.png`);
  let last = 'No window appeared';
  try {
    const deadline = Date.now() + paintTimeout;
    while (Date.now() < deadline) {
      assert.equal(child.exitCode, null, `Probe exited before pixels appeared: ${output}`);
      assert.equal(child.signalCode, null, `Probe terminated before pixels appeared: ${output}`);
      assert.doesNotMatch(output, /panicked at|pixel surface regression:/, output);
      if (!output.includes(scenarioComplete)) {
        last = `Waiting for ${scenarioComplete}`;
        await delay(pollInterval);
        continue;
      }
      const { windowID } = JSON.parse(run(helper, ['window', String(child.pid)]));
      if (windowID !== null) {
        if (control) {
          await verifyResize({ child, helper, windowID, control, directory });
          return;
        }
        run('/usr/sbin/screencapture', ['-x', '-o', '-l', String(windowID), screenshot]);
        const result = spawnSync(helper, ['analyze', screenshot, mode], {
          encoding: 'utf8',
          timeout: commandTimeout,
        });
        assert.ifError(result.error);
        assert.ok(result.status === 0 || result.status === 1, result.stderr);
        last = result.stdout.trim();
        if (result.status === 0) {
          const evidence = {
            ...JSON.parse(last),
            pid: child.pid,
            windowID,
            scenarioComplete: true,
          };
          await writeFile(join(directory, `${mode}-pixels.json`), `${JSON.stringify(evidence)}\n`);
          return;
        }
      }
      await delay(pollInterval);
    }
    assert.fail(`${mode} never displayed expected pixels for PID ${child.pid}: ${last}\n${output}`);
  } finally {
    await stop(child, exited);
    await writeFile(join(directory, `${mode}${resize ? '-resize' : ''}.log`), output);
  }
}

async function verifyResize({ child, helper, windowID, control, directory }) {
  for (const [index, state] of ['expanded', 'collapsed', 'expanded', 'collapsed'].entries()) {
    const next = `${control}.next`;
    await writeFile(next, state);
    await rename(next, control);
    const screenshot = join(directory, `resize-${index}-${state}.png`);
    const deadline = Date.now() + paintTimeout;
    let last = 'No resized pixels appeared';
    while (Date.now() < deadline) {
      assert.equal(child.exitCode, null, 'Resize probe exited early');
      assert.equal(child.signalCode, null, 'Resize probe terminated early');
      run('/usr/sbin/screencapture', ['-x', '-o', '-l', String(windowID), screenshot]);
      const result = spawnSync(
        helper,
        ['analyze-resize', screenshot, state === 'expanded' ? '224' : '56'],
        {
          encoding: 'utf8',
          timeout: commandTimeout,
        },
      );
      assert.ifError(result.error);
      assert.ok(result.status === 0 || result.status === 1, result.stderr);
      last = result.stdout.trim();
      if (result.status === 0) {
        await writeFile(join(directory, `resize-${index}-${state}.json`), `${last}\n`);
        break;
      }
      await delay(pollInterval);
    }
    assert.equal(JSON.parse(last).passed, true, `${state} retained stale video bounds: ${last}`);
  }
}

async function stop(child, exited) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill('SIGTERM');
  const stopped = await Promise.race([
    exited.then(() => true),
    delay(shutdownTimeout, undefined, { ref: false }).then(() => false),
  ]);
  if (!stopped) {
    child.kill('SIGKILL');
    await exited;
  }
}
