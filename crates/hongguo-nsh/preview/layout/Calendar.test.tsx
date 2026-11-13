import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('new drama calendar uses production dates and rows ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('text:00:42 / 02:49');
    await app.press('id:main-nav >> role:button[新剧]');
    await app.press('role:button[上新日历]');
    await app.waitFor('id:new-calendar-list >> text:归途有风');
    expect(app.shows('role:radio[今天 11/11]')).toBe(true);
    expect(app.texts().filter((text) => /^\d{2}-\d{2} \d{2}:\d{2}$/.test(text)).length).toBe(3);
    expect(app.shows('id:new-calendar-list >> text:9.2')).toBe(true);
    await app.hover('id:app-toolbar-rest');
    await app.wait(150);
    await app.screenshot('target/nsh-preview/new-' + scheme + '-calendar-1280x840.png');
    await app.press('id:new-calendar-list >> role:button[预约]');
    expect(app.count('id:new-calendar-list >> role:button[已预约]')).toBe(2);
    await app.press('text:11/12');
    await app.waitFor('text:这一天没有上新');
    await app.press('text:11/11');
    await app.waitFor('id:new-calendar-list >> text:归途有风');
    await app.press('id:new-calendar-list >> role:button[装穷提亲，娶到全家嫌弃的她]');
    await app.waitFor('id:detail-scroll >> text:剧情简介');
  });
}
