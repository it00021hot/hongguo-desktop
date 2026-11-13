import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('narrow production player chrome ' + scheme, async () => {
    const app = await open('.', { width: 480, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('text:00:42');
    await app.waitFor('text:02:49');
    expect(app.shows('id:player-stage')).toBe(true);
    expect(app.shows('text:00:42 / 02:49')).toBe(false);
    expect(app.spanStyle('text:00:42').fontSize).toBe(10);
    expect(app.spanStyle('text:00:42').fontWeight).toBe(400);
    expect(app.spanStyle('text:1x').fontSize).toBe(12);
    expect(app.shows('role:button[展开]')).toBe(false);
    await app.hover('id:app-toolbar-rest');
    await app.wait(200);
    await app.screenshot('target/nsh-preview/home-' + scheme + '-expanded-480x840.png');
  });
}
