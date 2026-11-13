import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('danmaku finishing layout desktop settings ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('text:00:42 / 02:49');
    await app.press('role:button[弹幕设置]');
    for (const name of ['不透明度', '字体大小', '密度', '显示区域']) {
      await app.waitFor('role:adjustable[' + name + ']');
    }
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-preview/danmaku-settings-' + scheme + '.png');
    await app.drag('id:hongguo.danmakuOpacity', { x: 220, y: 0 });
    await app.waitFor('text:100%');
    await app.keyboard.press('escape');
    await app.wait(180);
    expect(app.shows('role:adjustable[不透明度]')).toBe(false);
    await app.press('role:button[弹幕设置]');
    await app.waitFor('text:100%');
    await app.screenshot('target/nsh-preview/danmaku-settings-' + scheme + '-changed.png');
  });
}

test('danmaku finishing pending native wall clock pauses independently of two times media', async () => {
  const app = await open('.', { width: 1280, height: 840, colorScheme: 'dark', locale: 'zh-CN' });
  await app.waitFor('text:00:42 / 02:49');
  await app.keyboard.press('mod+shift+d');
  const caption = 'id:danmaku-motion-preview >> text:墙钟弹幕\uFFFC';
  await app.waitFor(caption);
  await app.press('role:button[暂停]');
  await app.waitFor('role:button[播放]');
  expect(app.shows(caption)).toBe(true);
  await app.wait(11000);
  expect(app.shows(caption)).toBe(true);
  await app.screenshot('target/nsh-preview/danmaku-motion-paused-2x.png');
  await app.press('role:button[播放]');
  await app.waitFor('role:button[暂停]');
  await app.wait(9000);
  expect(app.shows(caption)).toBe(false);
  await app.press('role:button[暂停]');
  await app.waitFor('role:button[播放]');
  await app.press('role:button[倍速]');
  await app.waitFor('role:menuitemradio[1.5x]');
  await app.press('role:menuitemradio[1.5x]');
  await app.waitFor('role:button[倍速] >> text:1.5x');
}, 60_000);
