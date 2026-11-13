import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'danmaku finishing layout mobile more ' + scheme,
    async () => {
      const app = await open('.', {
        width: 480,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42');
      await app.press('role:button[倍速]');
      await app.waitFor('id:mobile-player-more');
      expect(app.count('role:radio[1080P]')).toBe(1);
      expect(app.texts().some((text) => /\d+×\d+/.test(text))).toBe(false);
      expect(app.enabled('role:button[画质增强]')).toBe(false);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/player-more-' + scheme + '-480x840.png');
      await app.press('role:radio[1.5x]');
      await app.waitFor('role:button[倍速] >> text:1.5x');
      await app.keyboard.press('right');
      await app.waitFor('role:button[倍速] >> text:2x');
      await app.press('role:radio[1080P]');
      await app.press('role:radio[自动]');
      await app.press('role:switch[弹幕]');
      await app.press('role:button[弹幕设置]');
      await app.waitFor('role:adjustable[不透明度]');
      expect(app.count('role:adjustable[字体大小]')).toBe(1);
      expect(app.count('role:adjustable[密度]')).toBe(1);
      expect(app.count('role:adjustable[显示区域]')).toBe(1);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/player-more-' + scheme + '-expanded-480x840.png');
      await app.keyboard.press('escape');
      await app.press('role:button[倍速]');
      await app.waitFor('id:mobile-player-more');
      expect(app.shows('role:adjustable[不透明度]')).toBe(false);
      await app.press('role:button[选集 · 全80集]');
      await app.waitFor('id:player-episode-panel');
      expect(app.shows('id:mobile-player-more')).toBe(false);
    },
    60_000,
  );
}
