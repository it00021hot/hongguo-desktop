import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'player transport menus use the production view ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42 / 02:49');
      await app.press('role:button[倍速]');
      await app.waitFor('role:menuitemradio[1.5x]');
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/player-menu-rate-' + scheme + '-1280x840.png');
      await app.keyboard.press('escape');

      await app.press('role:button[清晰度]');
      await app.waitFor('role:menuitemradio[1080P]');
      expect(app.texts().some((text) => /\d+×\d+/.test(text))).toBe(false);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/player-menu-quality-' + scheme + '-1280x840.png');
      await app.keyboard.press('escape');

      await app.press('role:button[选集 · 全80集]');
      await app.waitFor('id:player-episode-panel');
      expect(app.count('id:player-episode-panel')).toBe(1);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/player-menu-episodes-' + scheme + '-1280x840.png');
      await app.press('role:button[系列剧]');
      await app.waitFor('id:player-season-list');
      await app.hover('id:app-toolbar-rest');
      await app.screenshot('target/nsh-preview/player-menu-seasons-' + scheme + '-1280x840.png');
      const reservation = 'role:button[预约 · 装穷提亲，娶到全家嫌弃的她 第三季]';
      await app.press(reservation);
      expect(app.enabled(reservation)).toBe(false);
      await app.press('role:button[选集]');
      await app.press('role:radio[31-60]');
      await app.press('role:button[第31集]');
      expect(app.shows('id:player-episode-panel')).toBe(true);
      await app.keyboard.press('escape');
      await app.waitFor('text:00:00 / 02:49');

      await app.press('role:button[倍速]');
      await app.press('role:menuitemradio[1.5x]');
      await app.waitFor('text:1.5x');
      await app.press('role:button[清晰度]');
      await app.press('role:menuitemradio[1080P]');
      await app.waitFor('text:1080P');

      await app.press('role:button[音量]');
      await app.waitFor('id:player-volume-panel');
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/player-menu-volume-' + scheme + '-1280x840.png');
      await app.press('role:button[静音]');
      await app.waitFor('role:button[取消静音]');
      await app.keyboard.press('escape');
      await app.press('role:button[选集 · 全80集]');
      await app.press('role:button[系列剧]');
      await app.press('id:player-season-list >> role:button[装穷提亲，娶到全家嫌弃的她 第二季]');
      expect(app.shows('id:player-episode-panel')).toBe(false);
      await app.waitFor('text:@装穷提亲，娶到全家嫌弃的她 第二季');
    },
    60_000,
  );
}
