import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  for (const [page, label] of [
    ['collections', '收藏'],
    ['liked', '点赞'],
    ['follows', '追更'],
    ['reservations', '预约'],
  ] as const) {
    test('personal production presentation ' + page + ' ' + scheme, async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42 / 02:49');
      await app.press('id:main-nav >> role:button[' + label + ']');
      await app.waitFor('text:归途有风');
      expect(app.shows('text:离线测试账号')).toBe(true);
      await app.hover('id:app-toolbar-rest');
      await app.wait(150);
      await app.screenshot('target/nsh-preview/' + page + '-' + scheme + '-expanded-1280x840.png');
      await app.press('role:button[管理]');
      expect(app.shows('role:button[退出管理]')).toBe(true);
      await app.press('role:button[全选]');
      await app.waitFor('text:已选 3 项');
      expect(app.shows('role:button[取消全选]')).toBe(true);
      await app.hover('id:app-toolbar-rest');
      await app.screenshot('target/nsh-preview/' + page + '-' + scheme + '-manage-1280x840.png');
    });
  }
}
