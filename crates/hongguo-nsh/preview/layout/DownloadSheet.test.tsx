import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const colorScheme of ['dark', 'light'] as const) {
  test(
    'player download sheet keeps consistent content insets and selection ' + colorScheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        platform: 'macos',
        colorScheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42 / 02:49');
      await app.press('role:button[下载]');
      await app.waitFor('id:player-download-sheet');
      await app.waitFor('text:已选 1 集');
      await app.hover('id:app-toolbar-rest');
      await app.wait(250);
      await app.screenshot('target/nsh-preview/download-sheet-' + colorScheme + '-1280x840.png');
      await app.press('role:button[全选]');
      await app.waitFor('text:已选 80 集');
      await app.press('role:button[反选]');
      await app.waitFor('text:已选 0 集');
      expect(app.enabled('role:button[开始下载]')).toBe(false);
      await app.type('id:download-range', '1-3,5，7');
      await app.waitFor('text:已选 5 集');
      await app.press('role:button[格式化选集]');
      expect(app.value('id:download-range')).toBe('1-3, 5, 7');
      await app.keyboard.press('escape');
      await app.waitFor('id:player-download-sheet', { state: 'gone' });
      await app.press('role:button[下载]');
      await app.waitFor('text:已选 1 集');
      expect(app.value('id:download-range')).toBe('');
      await app.press('role:button[取消]');
      await app.waitFor('id:player-download-sheet', { state: 'gone' });
    },
    60_000,
  );
}
