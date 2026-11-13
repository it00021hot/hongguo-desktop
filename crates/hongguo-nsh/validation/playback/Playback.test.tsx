import { expect, open, test as registerTest } from 'nsh/test';
import { episode, sameEpisode, seekAndConfirm, timeline, waitForPlaying } from './helpers';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

test('production playback starts, pauses, changes rate, switches episodes and resumes', async () => {
  const app = await open('.', {
    width: 1280,
    height: 840,
    colorScheme: 'dark',
    locale: 'zh-CN',
    network: 'live',
  });
  const steps: Record<string, unknown> = {};
  const start = Date.now();
  try {
    await waitForPlaying(app);
    const initialEpisode = episode(app);
    const initialTime = timeline(app)!.current;
    await waitForPlaying(
      app,
      () => {
        sameEpisode(app, initialEpisode);
        return (timeline(app)?.current ?? 0) >= initialTime + 2;
      },
      10_000,
    );
    expect(app.shows('id:player-stage')).toBe(true);
    steps.startup = {
      elapsedMs: Date.now() - start,
      timeline: timeline(app),
      episode: episode(app),
    };
    console.log('PLAYBACK_STEP_JSON ' + JSON.stringify({ startup: steps.startup }));
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-playback/playing.png');

    await app.press('role:button[暂停视频]');
    await app.waitFor('role:button[播放视频]');
    await app.wait(750);
    const paused = timeline(app)!.current;
    await app.wait(2200);
    expect(timeline(app)!.current).toBe(paused);
    steps.pause = { before: paused, after: timeline(app)!.current };
    await app.press('role:button[播放视频]');

    await app.press('role:button[倍速]');
    await app.press('role:menuitemradio[1.5x]');
    const rateStart = timeline(app)!.current;
    const wallStart = Date.now();
    await app.wait(4000);
    const ratio = (timeline(app)!.current - rateStart) / ((Date.now() - wallStart) / 1000);
    expect(ratio >= 1.2 && ratio <= 1.8).toBe(true);
    steps.rate = { requested: 1.5, measured: ratio, timeline: timeline(app) };
    console.log('PLAYBACK_STEP_JSON ' + JSON.stringify({ pause: steps.pause, rate: steps.rate }));

    const firstEpisode = episode(app);
    expect(app.enabled('role:button[下一集]')).toBe(true);
    await app.press('role:button[下一集]');
    await waitForPlaying(
      app,
      () => episode(app) !== firstEpisode && (timeline(app)?.current ?? 0) >= 2,
    );
    steps.next = { previous: firstEpisode, current: episode(app), timeline: timeline(app) };
    console.log('PLAYBACK_STEP_JSON ' + JSON.stringify({ next: steps.next }));
    expect(app.shows('text:1.5x')).toBe(true);
    await app.wait(2000);
    await app.press('role:button[暂停]');
    const resumeAt = timeline(app)!.current;
    const title = app
      .texts()
      .find((text) => text.startsWith('@'))!
      .slice(1);
    await app.press('id:main-nav >> role:button[历史]');
    await app.waitFor('text:' + title, { timeout: 10_000 });
    await app.press('text:' + title);
    // The displayed saved point alone is not evidence of a successful native seek.
    // Require playback to advance beyond it after reopening the video.
    await waitForPlaying(app, () => (timeline(app)?.current ?? 0) >= resumeAt + 2);
    expect(timeline(app)!.current <= resumeAt + 8).toBe(true);
    steps.resume = { saved: resumeAt, actual: timeline(app), episode: episode(app) };
    console.log('PLAYBACK_STEP_JSON ' + JSON.stringify({ resume: steps.resume }));
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-playback/resumed.png');

    await app.press('role:button[音量]');
    await app.press('role:button[静音]');
    expect(app.shows('role:button[取消静音]')).toBe(true);
    await app.press('role:button[取消静音]');
    expect(app.shows('role:button[静音]')).toBe(true);
    await app.keyboard.press('escape');
    steps.mute = { toggled: true, audibleOutputMeasured: false };

    await app.press('role:button[清晰度]');
    expect(app.shows('role:menuitemradio[自动] >> text:自动')).toBe(true);
    const offered = [480, 540, 360, 720, 1080].filter((value) =>
      app.shows('role:menuitemradio[' + value + 'P]'),
    );
    const requested = offered[0];
    expect(requested !== undefined).toBe(true);
    const qualityEpisode = episode(app);
    const qualityAt = timeline(app)!.current;
    await app.press('role:menuitemradio[' + requested + 'P]');
    await waitForPlaying(
      app,
      () =>
        app.enabled('role:button[清晰度]') &&
        app.shows('role:button[清晰度] >> text:' + requested + 'P'),
    );
    const manualBase = Math.max(qualityAt, timeline(app)!.current);
    await waitForPlaying(app, () => {
      sameEpisode(app, qualityEpisode);
      return (timeline(app)?.current ?? 0) >= manualBase + 2;
    });
    await app.press('role:button[清晰度]');
    expect(app.shows('role:menuitemradio[' + requested + 'P]')).toBe(true);
    const autoAt = timeline(app)!.current;
    await app.press('role:menuitemradio[自动]');
    await waitForPlaying(
      app,
      () => app.enabled('role:button[清晰度]') && app.shows('role:button[清晰度] >> text:自动'),
    );
    const autoBase = Math.max(autoAt, timeline(app)!.current);
    await waitForPlaying(app, () => {
      sameEpisode(app, qualityEpisode);
      return (timeline(app)?.current ?? 0) >= autoBase + 2;
    });
    steps.quality = {
      previousMode: 'automatic',
      requested,
      timeline: timeline(app),
      episode: qualityEpisode,
    };

    steps.seek = await seekAndConfirm(app);
  } catch (error) {
    steps.failure = { message: String(error), texts: app.texts(), timeline: timeline(app) };
    await app.screenshot('target/nsh-playback/failed.png');
    throw error;
  } finally {
    console.log('PLAYBACK_VERIFICATION_JSON ' + JSON.stringify(steps));
  }
}, 360_000);
