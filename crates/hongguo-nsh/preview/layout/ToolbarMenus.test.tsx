import { expect, open, test as registerTest } from 'nsh/test';
const test: (name: string, body: () => Promise<void>) => void = registerTest;
for (const scheme of ['dark', 'light'] as const) {
  test('tauri alignment: toolbar language and appearance menus ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      platform: 'macos',
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.waitFor('id:app-titlebar');
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-preview/toolbar-pin-off-' + scheme + '.png');
    await app.press('id:app-titlebar >> role:button[置顶]');
    await app.waitFor('role:button[取消置顶]');
    await app.hover('id:app-toolbar-rest');
    await app.wait(200);
    await app.screenshot('target/nsh-preview/toolbar-pin-on-' + scheme + '.png');
    await app.press('id:app-titlebar >> role:button[取消置顶]');
    await app.waitFor('id:app-titlebar >> role:button[置顶]');
    await app.press('id:app-titlebar >> role:button[语言]');
    await app.waitFor('role:menuitem[English]');
    await app.screenshot('target/nsh-preview/toolbar-language-' + scheme + '.png');
    await app.press('role:menuitem[English]');
    await app.waitFor('id:main-nav >> role:button[Settings]');
    await app.press('id:app-titlebar >> role:button[Language]');
    await app.press('role:menuitem[Simplified Chinese]');
    await app.waitFor('id:main-nav >> role:button[设置]');
    await app.press('id:app-titlebar >> role:button[切换主题]');
    await app.waitFor('role:menuitem[跟随系统]');
    await app.screenshot('target/nsh-preview/toolbar-appearance-' + scheme + '.png');
    await app.press('role:menuitem[亮色]');
    expect(app.shows('role:menuitem[亮色]')).toBe(false);
  });
}
