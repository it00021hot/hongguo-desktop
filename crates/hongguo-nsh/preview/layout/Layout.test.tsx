import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  for (const collapsed of [false, true]) {
    test('desktop home fixture ' + scheme + (collapsed ? ' collapsed' : ' expanded'), async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42 / 02:49');
      expect(app.shows('id:player-stage')).toBe(true);
      const navigationStyle = app.spanStyle('id:main-nav >> text:找剧');
      expect(navigationStyle.fontWeight).toBe(400);
      expect(navigationStyle.fontSize).toBe(14);
      if (collapsed) await app.press('role:button[收起]');
      await app.hover('id:app-toolbar-rest');
      await app.wait(250);
      await app.screenshot(
        'target/nsh-preview/home-' +
          scheme +
          '-' +
          (collapsed ? 'collapsed' : 'expanded') +
          '-1280x840.png',
      );
    });
  }
}
