import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('follow reminders presentation ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.waitFor('id:main-nav');
    await app.press('id:main-nav >> role:button[追更]');
    await app.waitFor('role:button[更新提醒]');
    await app.press('role:button[更新提醒]');
    await app.waitFor('text:暂无提醒');
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-followup-20261113/follow-notices-' + scheme + '-empty.png');
    await app.press('role:button[检查更新]');
    await app.waitFor('text:有 3 条新提醒');
    await app.waitFor('role:button[城市来信 · 预约的剧已上线，共 60 集 · 预约上线 · 未读]');
    expect(app.shows('id:follow-notices-unread >> text:2')).toBe(true);
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-followup-20261113/follow-notices-' + scheme + '-nonempty.png');
    await app.press('role:button[清空]');
    await app.waitFor('text:已清空');
    await app.waitFor('text:暂无提醒');
    expect(app.shows('id:follow-notices-unread')).toBe(false);
    await app.press('role:button[追更的剧]');
    await app.waitFor('role:button[管理]');
  });
}
