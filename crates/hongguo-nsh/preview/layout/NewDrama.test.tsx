import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('desktop new drama uses production cards ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.waitFor('text:00:42 / 02:49');
    await app.press('id:main-nav >> role:button[新剧]');
    await app.waitFor('id:new-recommend-grid >> text:归途有风');
    expect(app.shows('role:button[新剧推荐]')).toBe(true);
    expect(app.shows('role:button[上新日历]')).toBe(true);
    await app.press('role:button[男频]');
    expect(app.shows('id:new-recommend-grid >> text:归途有风')).toBe(true);
    await app.press('role:button[全部]');
    await app.hover('id:app-toolbar-rest');
    await app.wait(150);
    await app.screenshot('target/nsh-preview/new-' + scheme + '-expanded-1280x840.png');
    await app.press('role:button[上新日历]');
    await app.waitFor('id:new-calendar-list >> text:归途有风');
    await app.press('role:button[新剧推荐]');
    await app.waitFor('id:new-recommend-grid >> text:归途有风');
    await app.press('id:new-recommend-grid >> role:button[装穷提亲，娶到全家嫌弃的她]');
    await app.waitFor('id:detail-scroll >> text:第1集');
  });
}
