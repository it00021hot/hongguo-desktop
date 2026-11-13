import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  for (const [page, label] of [
    ['follows', '追更'],
    ['collections', '收藏'],
  ] as const) {
    test('desktop offline program library ' + page + ' ' + scheme, async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('id:main-nav');
      await app.press('id:main-nav >> role:button[' + label + ']');
      await app.waitFor('text:装穷提亲，娶到全家嫌弃的她');
      await app.waitFor('text:下一段故事');
      await app.waitFor('text:归途有风');
      expect(app.shows('text:离线测试账号')).toBe(true);
      expect(app.shows('role:button[管理]')).toBe(true);
      await app.hover('id:app-toolbar-rest');
      await app.wait(300);
      await app.screenshot(
        'target/nsh-preview/library-programs-' + page + '-' + scheme + '-1280x840.png',
      );
    });
  }
}
