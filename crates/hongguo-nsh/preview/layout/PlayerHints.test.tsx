import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('desktop corrections player icons and hover hints ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('text:00:42 / 02:49');
    await app.hover('role:button[弹幕设置]');
    await app.waitFor('text:弹幕设置');
    await app.screenshot('target/nsh-preview/player-hint-settings-' + scheme + '.png');
    await app.press('role:button[弹幕设置]');
    await app.waitFor('role:adjustable[不透明度]');
    await app.keyboard.press('escape');
    await app.press('role:button[关闭弹幕]');
    await app.waitFor('role:button[开启弹幕]');
    await app.hover('role:button[开启弹幕]');
    await app.waitFor('text:开启弹幕');
    await app.screenshot('target/nsh-preview/player-hint-danmaku-off-' + scheme + '.png');
    await app.press('role:button[开启弹幕]');
    await app.waitFor('role:button[关闭弹幕]');
    for (const name of ['播放', '下一集', '倍速', '清晰度', '下载', '全屏']) {
      await app.hover('role:button[' + name + ']');
      await app.waitFor('text:' + name);
    }
    expect(app.shows('role:button[关闭弹幕]')).toBe(true);
  });
}
