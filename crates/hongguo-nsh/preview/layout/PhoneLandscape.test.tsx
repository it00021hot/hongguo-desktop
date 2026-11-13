import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'phone landscape chrome preserves transport across confirmed orientation ' + scheme,
    async () => {
      const app = await open('.', {
        width: 390,
        height: 844,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('id:player-stage');
      await app.keyboard.press('mod+shift+w');
      await app.waitFor('id:window-preview-state');
      expect(app.shows('role:button[全屏观看]')).toBe(false);
      await app.keyboard.press('mod+alt+l');
      await app.waitFor('role:button[全屏观看]');
      await app.keyboard.press('mod+alt+v');
      await app.screenshot('target/nsh-preview/phone-landscape-entry-' + scheme + '.png');
      await app.press('role:button[全屏观看]');
      expect(app.shows('id:player-landscape-header')).toBe(false);
      await app.setViewport(844, 390);
      await app.keyboard.press('mod+alt+a');
      await app.waitFor('id:player-landscape-header');
      await app.waitFor('id:player-landscape-controls');
      expect(app.shows('id:bottom-nav')).toBe(false);
      expect(app.count('id:window-fixture-stage')).toBe(1);
      await app.press('id:player-landscape-controls >> role:button[倍速]');
      await app.waitFor('id:player-landscape-controls >> text:1.25x');
      await app.press('id:player-landscape-controls >> role:button[清晰度]');
      await app.waitFor('id:player-landscape-controls >> text:540P');
      await app.press('id:player-landscape-controls >> role:button[选集]');
      await app.waitFor('id:player-episode-panel');
      await app.keyboard.press('escape');
      await app.waitUntil(() => !app.shows('id:player-episode-panel'));
      await app.screenshot('target/nsh-preview/phone-landscape-controls-' + scheme + '.png');
      await app.press('role:button[退出全屏]');
      await app.keyboard.press('mod+alt+a');
      await app.setViewport(390, 844);
      await app.waitFor('role:button[全屏观看]');
      expect(app.shows('id:player-landscape-header')).toBe(false);
      expect(app.shows('role:button[倍速] >> text:1.25x')).toBe(true);
      expect(app.count('id:window-fixture-stage')).toBe(1);
    },
    60_000,
  );
}
