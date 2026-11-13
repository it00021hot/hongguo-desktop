import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const [mode, scheme, label] of [
  ['fullscreen', 'dark', '全屏'],
  ['frameless', 'light', '无边框窗口'],
] as const) {
  test('desktop fullscreen trailing controls ' + mode, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      platform: 'macos',
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.waitFor('id:player-stage');
    // Return keyboard focus from the native host to the app before the fixture shortcut.
    await app.press('role:button[倍速]');
    await app.waitFor('text:1.5x');
    await app.keyboard.press('escape');
    await app.keyboard.press('mod+shift+w');
    await app.waitFor('id:window-fixture-stage');
    await app.waitFor('id:main-nav');
    await app.press('role:button[' + label + ']');
    await app.keyboard.press('mod+alt+a');
    await app.waitFor('id:app-titlebar', { state: 'gone' });
    await app.waitFor('id:main-nav', { state: 'gone' });
    expect(app.count('id:window-fixture-stage')).toBe(1);
    expect(app.shows('role:button[全屏]')).toBe(true);
    await app.keyboard.press('mod+alt+v');
    await app.hover('id:window-fixture-stage');
    await app.screenshot('target/nsh-preview/window-' + mode + '-trailing-' + scheme + '.png');
    await app.press('role:button[' + label + ']');
    await app.keyboard.press('mod+alt+a');
    await app.waitFor('id:app-titlebar');
    await app.waitFor('id:main-nav');
    expect(app.count('id:window-fixture-stage')).toBe(1);
  });
}
