import { expect, open, test as registerTest } from 'nsh/test';
import { episode, sameEpisode, timeline, waitForPlaying } from './helpers';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

test('video surface pauses and resumes the real native clock', async () => {
  const app = await open('.', {
    width: 1280,
    height: 840,
    colorScheme: 'dark',
    locale: 'zh-CN',
    network: 'live',
  });
  try {
    await waitForPlaying(app, undefined, 45_000);
    const original = episode(app);
    await app.press('role:button[暂停视频]');
    await app.waitFor('role:button[播放视频]', { timeout: 3000 });
    await app.wait(750);
    const paused = timeline(app)!;
    await app.wait(2200);
    sameEpisode(app, original);
    expect(timeline(app)!.current).toBe(paused.current);
    await app.screenshot('target/nsh-playback/video-surface-paused.png');
    await app.press('role:button[播放视频]');
    await waitForPlaying(
      app,
      () => {
        sameEpisode(app, original);
        return (timeline(app)?.current ?? 0) >= paused.current + 2;
      },
      8000,
    );
    await app.screenshot('target/nsh-playback/video-surface-resumed.png');
    console.log(
      'VIDEO_SURFACE_JSON ' + JSON.stringify({ paused, resumed: timeline(app), episode: original }),
    );
  } catch (error) {
    await app.screenshot('target/nsh-playback/video-surface-failed.png');
    console.log(
      'VIDEO_SURFACE_FAILURE_JSON ' +
        JSON.stringify({ error: String(error), texts: app.texts(), timeline: timeline(app) }),
    );
    throw error;
  }
}, 70_000);
