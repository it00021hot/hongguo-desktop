import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const wallSeconds = Number(process.argv[2] ?? 8);
if (!Number.isFinite(wallSeconds) || wallSeconds < 1 || wallSeconds > 60) {
  throw new Error('用法：node scripts/bytevc2-playback-audit.mjs [每档测试秒数，1–60]');
}

async function audit(seconds) {
  // 选择器与事件名对着 src/features/player 里的直播放画布（HEVC / ByteVC1 / ByteVC2 共用）。
  const canvas = document.querySelector('canvas[data-direct-video]');
  const audio = canvas?.parentElement?.querySelector('video');
  if (!canvas || !audio || !canvas.dataset.renderedFrames || !Number.isFinite(audio.duration)) {
    throw new Error('请先在 Windows Tauri 打开内置软解直播放并等待出画');
  }
  if (audio.duration < seconds * 3 + 2) {
    throw new Error('片段太短，请缩短每档测试时间');
  }

  const original = { time: audio.currentTime, rate: audio.playbackRate, paused: audio.paused };
  const frameEvent = 'hongguo-direct-frame';
  const counter = (key) => Number(canvas.dataset[key] ?? 0);
  const clock = () => ({
    audio: audio.currentTime,
    rendered: counter('renderedFrames'),
    dropped: counter('droppedFrames'),
    rebuffers: counter('rebuffers'),
  });
  const waitForFrame = (target) =>
    new Promise((resolve, reject) => {
      const started = performance.now();
      const listener = () => {
        const pts = Number(canvas.dataset.videoPts);
        if (Math.abs(pts - target) > 0.15) return;
        clearTimeout(timer);
        canvas.removeEventListener(frameEvent, listener);
        resolve({ target, pts, ms: Math.round(performance.now() - started) });
      };
      const timer = setTimeout(() => {
        canvas.removeEventListener(frameEvent, listener);
        reject(new Error(`跳到 ${target} 秒后未显示对应的 ByteVC2 帧`));
      }, 15000);
      canvas.addEventListener(frameEvent, listener);
    });
  const seek = async (target) => {
    const waiting = waitForFrame(target);
    audio.currentTime = target;
    const frame = await waiting;
    const deadline = performance.now() + 15000;
    while (canvas.dataset.buffering === 'true' || audio.seeking) {
      if (performance.now() >= deadline) throw new Error('跳转后等待缓冲超时');
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    return frame;
  };

  try {
    audio.pause();
    const targets = [0.25, 0.75, 0.5].map((ratio) => audio.duration * ratio);
    for (const target of targets.slice(0, -1)) {
      audio.currentTime = target;
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    const rapidSeek = await seek(targets.at(-1));
    const rates = [];
    for (const rate of [1, 2, 3]) {
      audio.pause();
      await seek(0.5);
      audio.playbackRate = rate;
      await audio.play();
      const before = clock();
      const started = performance.now();
      const errors = [];
      const sample = () => errors.push(Math.abs(audio.currentTime - counter('videoPts')) * 1000);
      let displayCallbacks = 0;
      let animation;
      const displayTick = () => {
        displayCallbacks++;
        animation = requestAnimationFrame(displayTick);
      };
      animation = requestAnimationFrame(displayTick);
      canvas.addEventListener(frameEvent, sample);
      try {
        await new Promise((resolve) => setTimeout(resolve, seconds * 1000));
      } finally {
        cancelAnimationFrame(animation);
        canvas.removeEventListener(frameEvent, sample);
      }
      const after = clock();
      errors.sort((left, right) => left - right);
      rates.push({
        rate,
        elapsed: (performance.now() - started) / 1000,
        mediaAdvanced: after.audio - before.audio,
        rendered: after.rendered - before.rendered,
        dropped: after.dropped - before.dropped,
        displayCallbacks,
        rebuffers: after.rebuffers - before.rebuffers,
        ptsP95Ms: errors.length ? Math.round(errors[Math.ceil(errors.length * 0.95) - 1]) : null,
        error: audio.error?.message ?? null,
      });
    }
    return { width: canvas.width, height: canvas.height, rapidSeek, rates };
  } finally {
    audio.pause();
    audio.playbackRate = original.rate;
    if (Math.abs(audio.currentTime - original.time) > 0.01) {
      if (original.paused) audio.currentTime = original.time;
      else await seek(original.time);
    }
    if (!original.paused) await audio.play();
  }
}

const cdp = fileURLToPath(new URL('./cdp.mjs', import.meta.url));
execFileSync(process.execPath, [cdp, 'eval', `(${audit.toString()})(${wallSeconds})`], {
  stdio: 'inherit',
  timeout: (wallSeconds * 3 + 160) * 1000,
});
