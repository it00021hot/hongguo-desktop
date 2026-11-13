import { expect, open, test as registerTest, type App } from 'nsh/test';
import { episode, sameEpisode, timeline, waitForPlaying } from './helpers';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

function title(app: App) {
  return app.texts().find((text) => text.startsWith('@'));
}

async function selectLastEpisode(app: App) {
  const current = episode(app);
  const total = current?.match(/全(\d+)集$/)?.[1];
  if (!total) throw new Error('The current source has no episode count');
  await app.hover('id:player-stage');
  await app.press('role:button[选集 · 全' + total + '集]');
  await app.waitFor('id:player-episode-panel');
  const groups = app
    .texts()
    .filter((text) => /^\d+-\d+$/.test(text))
    .sort((a, b) => Number(a.split('-')[1]) - Number(b.split('-')[1]));
  const lastGroup = groups.at(-1);
  if (lastGroup) await app.press('role:radio[' + lastGroup + ']');
  const last = Array.from(
    new Set(
      app
        .texts()
        .filter((text) => /^\d+$/.test(text))
        .map(Number),
    ),
  )
    .sort((a, b) => b - a)
    .find((index) => app.shows('id:player-episode-grid >> role:button[第' + index + '集]'));
  if (!last) throw new Error('No actual episode button exists in the final group');
  await app.press('id:player-episode-grid >> role:button[第' + last + '集]');
  await app.keyboard.press('escape');
  await app.waitFor('id:player-episode-panel', { state: 'gone' });
  const expected = '第' + last + '集 · 全' + total + '集';
  await waitForPlaying(app, () => episode(app) === expected && (timeline(app)?.current ?? 0) >= 2);
  const first = timeline(app)!.current;
  await waitForPlaying(
    app,
    () => {
      sameEpisode(app, expected);
      return (timeline(app)?.current ?? 0) >= first + 2;
    },
    10_000,
  );
  return expected;
}

test('production relay advances only after a real final-episode end and new native progress', async () => {
  const app = await open('.', {
    width: 1280,
    height: 840,
    colorScheme: 'dark',
    locale: 'zh-CN',
    network: 'live',
  });
  const steps: Record<string, unknown> = {};
  try {
    await waitForPlaying(app);
    const finalEpisode = await selectLastEpisode(app);
    const previousTitle = title(app);
    if (!previousTitle) throw new Error('The final source has no visible drama title');
    await waitForPlaying(app, () => {
      sameEpisode(app, finalEpisode);
      return (
        (timeline(app)?.duration ?? 0) >= 8 && app.enabled('role:adjustable[Playback position]')
      );
    });
    await app.press('role:button[暂停视频]');
    await app.waitFor('role:button[播放视频]');
    await app.wait(750);
    const before = timeline(app)!;
    await app.wait(1200);
    expect(timeline(app)!.current).toBe(before.current);
    if (before.duration < 8) throw new Error('The final episode is too short for the end boundary');
    const target = before.duration - 3;
    // Use the actual 1024px rail at 1280px/224px sidebar, not its transformed thumb.
    await app.drag('id:player-seek-rail', { x: (target / before.duration - 0.5) * 1024, y: 0 });
    await waitForPlaying(
      app,
      () => {
        sameEpisode(app, finalEpisode);
        return Math.abs((timeline(app)?.current ?? Infinity) - target) <= 1;
      },
      120_000,
    );
    await app.waitFor('role:button[播放视频]');
    await app.waitUntil(() => !app.shows('text:缓冲中'));
    const sought = timeline(app)!;
    await app.wait(1200);
    expect(timeline(app)!.current).toBe(sought.current);
    sameEpisode(app, finalEpisode);
    expect(title(app)).toBe(previousTitle);
    steps.final = {
      episode: finalEpisode,
      title: previousTitle,
      before,
      target,
      confirmed: sought,
    };
    console.log('RELAY_STEP_JSON ' + JSON.stringify(steps));
    await app.press('role:button[播放视频]');
    await waitForPlaying(
      app,
      () =>
        Boolean(title(app) && title(app) !== previousTitle) &&
        episode(app) !== finalEpisode &&
        (timeline(app)?.current ?? 0) >= 2,
      120_000,
    );
    const relayedTitle = title(app);
    const relayedEpisode = episode(app);
    const relayed = timeline(app)!;
    await waitForPlaying(
      app,
      () => {
        sameEpisode(app, relayedEpisode);
        if (title(app) !== relayedTitle) throw new Error('Relay changed sources twice');
        return (timeline(app)?.current ?? 0) >= relayed.current + 2;
      },
      10_000,
    );
    steps.relayed = {
      title: relayedTitle,
      episode: relayedEpisode,
      first: relayed,
      later: timeline(app),
    };
    console.log('RELAY_VERIFICATION_JSON ' + JSON.stringify(steps));
    await app.press('role:button[暂停视频]');
    await app.waitFor('role:button[播放视频]');
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-playback/relay-confirmed.png');
  } catch (error) {
    console.log(
      'RELAY_FAILURE_JSON ' + JSON.stringify({ message: String(error), steps, texts: app.texts() }),
    );
    await app.screenshot('target/nsh-playback/relay-failed.png');
    throw error;
  }
}, 300_000);
