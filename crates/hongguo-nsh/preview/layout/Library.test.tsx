import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  for (const page of ['history', 'tasks'] as const) {
    test('desktop ' + page + ' uses the production list ' + scheme, async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42 / 02:49');
      await app.press(
        'id:main-nav >> role:button[' + (page === 'history' ? '历史' : '下载管理') + ']',
      );
      const marker = page === 'history' ? '看到第1集/共80集 00:42' : '4.0 MB / 8.0 MB';
      await app.waitFor('text:' + marker);
      expect(app.shows('text:' + marker)).toBe(true);
      await app.hover('id:app-toolbar-rest');
      await app.wait(150);
      await app.screenshot('target/nsh-preview/' + page + '-' + scheme + '-expanded-1280x840.png');
    });
  }
}

for (const scheme of ['dark', 'light'] as const) {
  test('narrow task table scrolls to its actual row actions ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('text:00:42 / 02:49');
    await app.press('id:main-nav >> role:button[下载管理]');
    await app.waitFor('text:4.0 MB / 8.0 MB');
    await app.setViewport(480, 840);
    await app.hover('id:app-toolbar-rest');
    await app.wait(250);
    await app.screenshot('target/nsh-preview/tasks-' + scheme + '-expanded-480x840.png');
    await app.scroll('id:task-table-horizontal', 200);
    await app.screenshot('target/nsh-preview/tasks-' + scheme + '-scrolled-480x840.png');
    await app.press('role:button[打开文件夹]');
    await app.waitFor('text:预览操作：打开文件夹 · fixture');
    await app.scroll('id:task-table-horizontal', -200);
    await app.screenshot('target/nsh-preview/tasks-' + scheme + '-returned-480x840.png');
  });
}
