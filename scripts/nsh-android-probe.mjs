import { spawn, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const replayOnly = args.includes('--replay-only');
const serial = args[args.indexOf('--serial') + 1];
if (!args.includes('--serial') || !serial?.startsWith('emulator-'))
  throw new Error('Pass --serial emulator-PORT for an explicitly selected test emulator');
const sdk = process.env.ANDROID_HOME;
if (!sdk) throw new Error('ANDROID_HOME is required');
const adb = join(sdk, 'platform-tools/adb');
const project = join(root, 'target/nsh-android/pixel-probe');
const output = join(root, 'target/nsh-pixel-android-probe', replayOnly ? 'replay' : '');
const packageId = 'com.hongguo.nsh.pixelprobe';
mkdirSync(output, { recursive: true });

function run(command, argv, options = {}) {
  const result = spawnSync(command, argv, { cwd: root, timeout: 300_000, ...options });
  if (result.error || result.status !== 0)
    throw new Error(
      `${command} failed: ${result.error?.message ?? result.stderr?.toString() ?? result.status}`,
    );
  return result.stdout;
}
function device(argv, options = {}) {
  return run(adb, ['-s', serial, ...argv], options);
}

if (!args.includes('--skip-build'))
  run(process.execPath, ['scripts/nsh.mjs', 'package', 'android'], { stdio: 'inherit' });
const library = join(root, 'target/nsh-android/jniLibs/arm64-v8a/libnsh.so');
if (!existsSync(library)) throw new Error('Build the Android host before --skip-build');
cpSync(join(root, 'crates/hongguo-nsh-video-android/tests/probe'), project, { recursive: true });
const assets = join(project, 'app/src/main/assets');
mkdirSync(assets, { recursive: true });
run('ffmpeg', [
  '-hide_banner',
  '-loglevel',
  'error',
  '-f',
  'lavfi',
  '-i',
  'testsrc2=size=320x180:rate=24',
  '-t',
  '30',
  '-c:v',
  'libx264',
  '-preset',
  'veryfast',
  '-g',
  '24',
  '-pix_fmt',
  'yuv420p',
  '-an',
  '-movflags',
  '+faststart',
  '-y',
  join(assets, 'input.mp4'),
]);
run(join(root, 'crates/hongguo-nsh-android/android/gradlew'), ['-p', project, 'assembleDebug'], {
  stdio: 'inherit',
});
device(['install', '-r', join(project, 'app/build/outputs/apk/debug/app-debug.apk')]);
device(['shell', 'am', 'force-stop', packageId]);
// Package replacement can briefly schedule the old activity's relaunch. Wait
// for this test package alone to stop before starting the fresh fixture.
for (let attempt = 0; attempt < 20; attempt++) {
  const state = spawnSync(adb, ['-s', serial, 'shell', 'pidof', packageId], { encoding: 'utf8' });
  if (!state.stdout.trim()) break;
  await new Promise((resolve) => setTimeout(resolve, 100));
  device(['shell', 'am', 'force-stop', packageId]);
}
device([
  'shell',
  'am',
  'start',
  '-W',
  '-n',
  `${packageId}/com.hongguo.nsh.PixelProbeActivity`,
  ...(replayOnly ? ['--ez', 'replayOnly', 'true'] : []),
]);
const pid = device(['shell', 'pidof', packageId]).toString().trim();
if (!/^\d+$/.test(pid)) throw new Error('The isolated probe did not start');
const reader = spawn(adb, [
  '-s',
  serial,
  'logcat',
  '-d',
  '--pid=' + pid,
  '-s',
  'HGPixelProbe',
  'AndroidRuntime',
  'nsh',
]);
let log = '';
reader.stdout.on('data', (data) => {
  log += data.toString();
});
await new Promise((resolve, reject) => {
  reader.on('error', reject);
  reader.on('close', (code) =>
    code === 0 ? resolve() : reject(new Error('Initial log read failed')),
  );
});
const capture = new Set();
const live = spawn(adb, [
  '-s',
  serial,
  'logcat',
  '--pid=' + pid,
  '-T',
  '1',
  '-s',
  'HGPixelProbe',
  'AndroidRuntime',
  'nsh',
]);
try {
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(
      () => reject(new Error('Native probe did not complete in 60 seconds')),
      60_000,
    );
    const examine = () => {
      writeFileSync(join(output, 'native.log'), log);
      if (
        /HGPixelProbe: FAIL|FATAL EXCEPTION|Dropping a GlobalRef in a detached thread|output detach failed/.test(
          log,
        )
      ) {
        clearTimeout(deadline);
        reject(new Error('Native probe failed; inspect target/nsh-pixel-android-probe/native.log'));
        return;
      }
      for (const [marker, name] of [
        ['PAUSED_GPU_FRAME_READY', 'paused'],
        ['ANIME_GPU_FRAME_READY', 'anime'],
        ['PLAYING_GPU_FRAME_READY', 'playing'],
      ]) {
        if (log.includes(marker) && !capture.has(name)) {
          capture.add(name);
          setTimeout(() => {
            const shot = spawnSync(adb, ['-s', serial, 'exec-out', 'screencap', '-p'], {
              timeout: 5000,
            });
            if (shot.status === 0) writeFileSync(join(output, `${name}.png`), shot.stdout);
          }, 300);
        }
      }
      if (
        (replayOnly
          ? /PASS replayAfterEnd=true pausedReplay=true samePlayer=true/
          : /PASS pausedGpu=true playingGpu=true animeGpu=true/
        ).test(log) &&
        log.includes('DESTROYED')
      ) {
        clearTimeout(deadline);
        resolve();
      }
    };
    live.stdout.on('data', (data) => {
      log += data.toString();
      examine();
    });
    live.on('error', (error) => {
      clearTimeout(deadline);
      reject(error);
    });
    examine();
  });
  await new Promise((resolve) => setTimeout(resolve, 500));
  const required = replayOnly
    ? []
    : [
        'raw',
        'paused-gpu',
        'anime-gpu',
        'playing-gpu',
        'source-2-gpu',
        'cover-gpu',
        'contain-gpu',
        'landscape-gpu',
        'restored-gpu',
        'reactivated-gpu',
      ];
  const pixels = [...log.matchAll(/PIXELS ([\w-]+) child=(\d).*?colored=(\d+)/g)].map((match) => ({
    stage: match[1],
    child: Number(match[2]),
    colored: Number(match[3]),
  }));
  for (const stage of required) {
    if (
      !pixels.some(
        (item) =>
          item.stage === stage && item.child === (stage === 'raw' ? 0 : 1) && item.colored >= 100,
      )
    )
      throw new Error(`Missing real nonblack texture assertion: ${stage}`);
    const bytes = device([
      'exec-out',
      'run-as',
      packageId,
      'cat',
      `files/${stage}-${stage === 'raw' ? 0 : 1}.png`,
    ]);
    writeFileSync(join(output, `${stage}-texture.png`), bytes);
  }
  const pass = log.split('\n').find((line) => line.includes('HGPixelProbe: PASS'));
  const summary = {
    serial,
    pid,
    sample: 'locally generated H264, 320x180, 24fps, 30000ms, no network permission',
    pass,
    pixels,
  };
  writeFileSync(join(output, 'summary.json'), JSON.stringify(summary, null, 2) + '\n');
  console.log(JSON.stringify(summary));
} finally {
  live.kill('SIGTERM');
  writeFileSync(join(output, 'native.log'), log);
  device(['shell', 'am', 'force-stop', packageId]);
}
