import { expect, open, test as registerTest } from 'nsh/test';
import { timeline, waitForPlaying } from './helpers';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

test('production navigation retains the playing feed across search and detail lifetimes', async () => {
  const app = await open('.', {
    width: 1280,
    height: 840,
    colorScheme: 'dark',
    locale: 'zh-CN',
    network: 'live',
  });
  const mark = (step: string) =>
    console.log('NAVIGATION_LIFETIME_JSON ' + JSON.stringify({ step, at: Date.now() }));
  try {
    mark('initial-feed');
    await app.waitFor('id:main-nav', { timeout: 30_000 });
    await waitForPlaying(app);
    const initial = timeline(app)!;
    await waitForPlaying(app, () => (timeline(app)?.current ?? 0) >= initial.current + 1);
    mark('open-browse');
    await app.press('id:main-nav >> role:button[找剧]');
    await app.waitFor('id:browse-query');
    mark('type-query');
    await app.type('id:browse-query', '持械入宋');
    mark('submit-search');
    await app.press('role:button[搜索]');
    await app.waitUntil(
      () =>
        app.shows('id:browse-results') && app.texts().some((text) => text.startsWith('持械入宋')),
      { timeout: 30_000 },
    );
    const title = app.texts().find((text) => text.startsWith('持械入宋'));
    if (!title) throw new Error('The returned search result has no drama title');
    mark('open-detail');
    await app.press('id:browse-results >> text:' + title + ' >> nth=0');
    await app.waitFor('id:detail-scroll');
    await app.waitUntil(() => app.texts().some((text) => text.startsWith(title)), {
      timeout: 30_000,
    });
    mark('back-to-browse');
    await app.press('role:button[后退]');
    await app.waitFor('id:browse-results');
    expect(app.value('id:browse-query')).toBe('持械入宋');
    mark('return-to-feed');
    await app.press('id:main-nav >> role:button[推荐]');
    await waitForPlaying(app);
    const restored = timeline(app)!;
    await waitForPlaying(app, () => (timeline(app)?.current ?? 0) >= restored.current + 2);
    mark('feed-healthy');
    await app.press('role:button[暂停视频]');
    await app.waitFor('role:button[播放视频]');
    await app.screenshot('target/nsh-playback/navigation-lifetime-healthy.png');
  } catch (error) {
    console.log(
      'NAVIGATION_LIFETIME_FAILURE_JSON ' +
        JSON.stringify({ message: String(error), texts: app.texts() }),
    );
    await app.screenshot('target/nsh-playback/navigation-lifetime-failed.png');
    throw error;
  }
}, 180_000);
