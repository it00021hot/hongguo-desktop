import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'shared appearance finishing ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42 / 02:49');
      await app.press('role:button[清晰度]');
      await app.waitFor('role:menuitemradio[720P]');
      expect(app.texts().some((text) => /\d+×\d+/.test(text))).toBe(false);
      await app.hover('id:app-toolbar-rest');
      await app.wait(160);
      await app.screenshot('target/nsh-preview/player-menu-quality-' + scheme + '-1280x840.png');
      await app.keyboard.press('escape');
      await app.press('role:button[选集 · 全80集]');
      await app.waitFor('id:player-episode-panel');
      await app.hover('id:app-toolbar-rest');
      await app.wait(160);
      await app.screenshot('target/nsh-preview/player-menu-episodes-' + scheme + '-1280x840.png');
      await app.press('role:button[系列剧]');
      await app.waitFor('id:player-season-list');
      await app.hover('id:app-toolbar-rest');
      await app.screenshot('target/nsh-preview/player-menu-seasons-' + scheme + '-1280x840.png');
      await app.keyboard.press('escape');
      await app.press('id:main-nav >> role:button[新剧]');
      await app.press('role:button[上新日历]');
      await app.waitFor('id:new-calendar-list >> text:归途有风');
      await app.hover('id:app-toolbar-rest');
      await app.wait(150);
      await app.screenshot('target/nsh-preview/new-' + scheme + '-calendar-1280x840.png');
    },
    60_000,
  );
}
