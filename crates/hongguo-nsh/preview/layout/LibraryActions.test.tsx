import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'settings and library finishing actions ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42 / 02:49');
      await app.press('id:main-nav >> role:button[历史]');
      await app.waitFor('text:看到第1集/共80集 00:42');
      await app.press('role:button[管理]');
      await app.press('role:checkbox[装穷提亲，娶到全家嫌弃的她]');
      await app.press('role:button[删除所选]');
      await app.waitFor('id:confirmation');
      await app.hover('id:confirmation');
      await app.wait(160);
      await app.screenshot('target/nsh-preview/history-delete-confirm-' + scheme + '-1280x840.png');
      await app.keyboard.press('escape');
      await app.press('role:button[删除所选]');
      await app.press('id:confirmation >> role:button[确定]');
      await app.waitFor('role:button[管理]');
      expect(app.shows('text:看到第1集/共80集 00:42')).toBe(false);
      await app.press('id:main-nav >> role:button[下载管理]');
      await app.waitFor('text:4.0 MB / 8.0 MB');
      await app.press('role:checkbox[preview-task-3]');
      await app.press('id:task-batch-actions >> role:button[删除]');
      await app.waitFor('id:confirmation');
      await app.press('role:checkbox[同时删除本地文件]');
      await app.hover('id:confirmation');
      await app.wait(160);
      await app.screenshot('target/nsh-preview/tasks-delete-confirm-' + scheme + '-1280x840.png');
      await app.press('id:confirmation >> role:button[删除任务和文件]');
      await app.waitFor('text:4.0 MB / 8.0 MB');
      expect(app.shows('role:checkbox[preview-task-3]')).toBe(false);
    },
    60_000,
  );
}
