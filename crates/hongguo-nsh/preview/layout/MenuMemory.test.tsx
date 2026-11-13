import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('view memory finishing browse draft filters and scroll ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 640,
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    const browse = 'id:main-nav >> role:button[找剧]';
    await app.press(browse);
    await app.waitFor('id:browse-results');
    await app.press('role:button[真人剧]');
    await app.type('id:browse-query', '保留草稿');
    await app.keyboard.press('escape');
    await app.scroll('id:form-scroll', 5000);
    await app.screenshot('target/nsh-followup-20261113/browse-scroll-before-' + scheme + '.png');
    await app.press('id:main-nav >> role:button[排行榜]');
    await app.waitFor('id:rank-header');
    await app.press(browse);
    await app.waitFor('id:browse-results');
    await app.screenshot('target/nsh-followup-20261113/browse-scroll-return-' + scheme + '.png');
    await app.scroll('id:form-scroll', -1);
    await app.scroll('id:form-scroll', -5000);
    expect(app.value('id:browse-query')).toBe('保留草稿');
    expect(app.shows('role:button[筛选] >> text:1')).toBe(true);
    await app.screenshot('target/nsh-followup-20261113/browse-memory-top-' + scheme + '.png');
  });
}
