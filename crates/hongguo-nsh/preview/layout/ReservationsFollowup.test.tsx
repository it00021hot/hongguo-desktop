import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('reservations anonymous toolbar followup ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.waitFor('id:main-nav');
    await app.press('id:main-nav >> role:button[预约]');
    await app.waitFor('text:归途有风');
    await app.keyboard.press('mod+shift+a');
    await app.waitFor('text:登录后才能查看预约列表');
    expect(app.shows('role:button[管理]')).toBe(false);
    expect(app.shows('role:button[已上线]')).toBe(true);
    expect(app.shows('role:button[待上线]')).toBe(true);
    await app.press('role:button[待上线]');
    await app.type('label:搜索预约的剧', '归途');
    expect(app.value('label:搜索预约的剧')).toBe('归途');
    expect(app.shows('text:归途有风')).toBe(false);
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-followup-20261113/reservations-anonymous-' + scheme + '.png');
    await app.press('role:button[待上线]');
    await app.keyboard.press('tab');
    await app.keyboard.press('mod+shift+a');
    await app.waitFor('role:button[管理]');
    await app.press('role:button[已上线  3]');
    await app.waitFor('text:归途有风');
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-followup-20261113/reservations-filtered-' + scheme + '.png');
  });
}
