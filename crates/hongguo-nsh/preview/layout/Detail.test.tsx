import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('desktop details use the production view ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('text:@装穷提亲，娶到全家嫌弃的她');
    await app.press('text:@装穷提亲，娶到全家嫌弃的她');
    await app.waitFor('id:detail-scroll >> text:第1集');
    expect(app.spanStyle('id:detail-scroll >> text:装穷提亲，娶到全家嫌弃的她').fontSize).toBe(24);
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-preview/detail-' + scheme + '-expanded-1280x840.png');
  });
}
