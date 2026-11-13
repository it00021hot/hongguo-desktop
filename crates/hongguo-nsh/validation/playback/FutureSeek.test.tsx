import { expect, open, test as registerTest } from 'nsh/test';
import {
  episode,
  sameEpisode,
  startPreviouslyFailingSource,
  timeline,
  waitForPlaying,
} from './helpers';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

test('production paused seek reaches a distant position within the same episode', async () => {
  const app = await open('.', {
    width: 1280,
    height: 840,
    colorScheme: 'dark',
    locale: 'zh-CN',
    network: 'live',
  });
  try {
    await startPreviouslyFailingSource(app);
    await waitForPlaying(app);
    console.log(
      'FUTURE_SEEK_STARTED_JSON ' +
        JSON.stringify({ observedAtMs: Date.now(), timeline: timeline(app) }),
    );
    await app.press('role:button[暂停]');
    const original = episode(app);
    const before = timeline(app)!;
    const expected = before.current + (before.duration * 640) / 1024;
    expect(expected < before.duration - 5).toBe(true);
    await app.hover('role:adjustable[Playback position]');
    await app.drag('role:adjustable[Playback position]', { x: 640, y: 0 });
    console.log(
      'FUTURE_SEEK_REQUESTED_JSON ' +
        JSON.stringify({ observedAtMs: Date.now(), before, expected, episode: original }),
    );
    await waitForPlaying(
      app,
      () => {
        sameEpisode(app, original);
        return Math.abs((timeline(app)?.current ?? Infinity) - expected) <= 2;
      },
      45_000,
    );
    const confirmed = timeline(app)!;
    await app.press('role:button[播放]');
    await waitForPlaying(
      app,
      () => {
        sameEpisode(app, original);
        return (timeline(app)?.current ?? 0) >= confirmed.current + 2;
      },
      10_000,
    );
    console.log(
      'FUTURE_SEEK_VERIFICATION_JSON ' +
        JSON.stringify({ before, expected, confirmed, actual: timeline(app), episode: original }),
    );
    await app.screenshot('target/nsh-playback/future-seek-confirmed.png');
  } catch (error) {
    console.log(
      'FUTURE_SEEK_FAILURE_JSON ' + JSON.stringify({ message: String(error), texts: app.texts() }),
    );
    await app.screenshot('target/nsh-playback/future-seek-failed.png');
    throw error;
  }
}, 180_000);
