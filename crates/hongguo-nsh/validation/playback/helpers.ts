import type { App } from 'nsh/test';

export function timeline(app: App) {
  const label = app.texts().find((text) => /^\d+:\d{2} \/ \d+:\d{2}$/.test(text));
  if (!label) return null;
  const [current, duration] = label.split(' / ').map((part) => {
    const [minutes, seconds] = part.split(':').map(Number);
    return minutes * 60 + seconds;
  });
  return { current, duration, label };
}

export function episode(app: App) {
  return app.texts().find((text) => /^第\d+集 · 全\d+集$/.test(text));
}

export async function waitForPlaying(
  app: App,
  predicate = () => (timeline(app)?.current ?? 0) >= 2,
  timeout = 120_000,
) {
  await app.waitUntil(
    () => {
      if (app.shows('id:player-error')) {
        throw new Error('Native playback failed: ' + app.texts().join(' | '));
      }
      return predicate();
    },
    { timeout },
  );
}

export function sameEpisode(app: App, expected: string | undefined) {
  if (!expected || episode(app) !== expected) {
    throw new Error('Playback changed episodes unexpectedly: ' + episode(app));
  }
}

export async function startPreviouslyFailingSource(app: App) {
  await app.waitFor('id:main-nav', { timeout: 30_000 });
  await app.press('id:main-nav >> role:button[找剧]');
  await app.type('id:browse-query', '持械入宋');
  await app.press('role:button[搜索]');
  await app.waitUntil(
    () => app.shows('id:browse-results') && app.texts().some((text) => text.startsWith('持械入宋')),
    { timeout: 30_000 },
  );
  const title = app.texts().find((text) => text.startsWith('持械入宋'));
  if (!title) throw new Error('The previously failing source is absent from search results');
  await app.press('id:browse-results >> text:' + title + ' >> nth=0');
  await app.waitFor('role:button[从第 1 集开始]', { timeout: 30_000 });
  const started = Date.now();
  await app.press('role:button[从第 1 集开始]');
  return started;
}

export async function seekAndConfirm(app: App) {
  const original = episode(app);
  await app.waitUntil(() => app.enabled('role:adjustable[Playback position]'), {
    timeout: 120_000,
  });
  await app.press('role:button[暂停]');
  await app.waitFor('role:button[播放视频]');
  await app.wait(750);
  const before = timeline(app)!;
  if (before.duration < 64)
    throw new Error('This pointer seek case needs a video longer than 64 seconds');
  // At 1280px the 224px sidebar and 16px rail padding leave 1024px.
  // Native test bounds omit the thumb's layer transform. Use the actual rail's
  // middle instead, where the user presses before dragging in either direction.
  const rail = 'id:player-seek-rail';
  const pixels = before.current < before.duration / 2 ? 64 : -64;
  const expected = (0.5 + pixels / 1024) * before.duration;
  const expectedBack = (0.5 - pixels / 1024) * before.duration;
  await app.hover(rail);
  await app.drag(rail, { x: pixels, y: 0 });
  await waitForPlaying(
    app,
    () => {
      sameEpisode(app, original);
      return Math.abs((timeline(app)?.current ?? Infinity) - expected) <= 2;
    },
    10_000,
  );
  await app.wait(500);
  const forward = timeline(app)!;
  await app.wait(1200);
  if (timeline(app)?.current !== forward.current)
    throw new Error('Native playback did not remain paused after seeking');
  if (app.shows('text:缓冲中')) throw new Error('Seek confirmation left a buffering indicator');
  await app.drag(rail, { x: -pixels, y: 0 });
  await waitForPlaying(
    app,
    () => {
      sameEpisode(app, original);
      return Math.abs((timeline(app)?.current ?? Infinity) - expectedBack) <= 2;
    },
    10_000,
  );
  await app.wait(500);
  const backward = timeline(app)!;
  await app.wait(1200);
  if (timeline(app)?.current !== backward.current)
    throw new Error('Native playback did not remain paused after seeking backwards');
  if (app.shows('text:缓冲中'))
    throw new Error('Backwards seek confirmation left a buffering indicator');
  await app.press('role:button[播放]');
  await waitForPlaying(
    app,
    () => {
      sameEpisode(app, original);
      return (timeline(app)?.current ?? 0) >= backward.current + 2;
    },
    10_000,
  );
  return {
    pixels,
    expected,
    expectedBack,
    before,
    forward,
    backward,
    actual: timeline(app),
    episode: original,
  };
}
