import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const colorScheme of ['dark', 'light'] as const) {
  test(
    'maintenance merge page ' + colorScheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        platform: 'macos',
        colorScheme,
        locale: 'zh-CN',
      });
      await app.press('id:main-nav >> role:button[一键合并]');
      await app.waitFor('id:merge-series');
      expect(app.enabled('role:button[开始合并]')).toBe(false);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/merge-' + colorScheme + '-expanded-1280x840.png');
      await app.press('role:combobox[选择剧集]');
      await app.keyboard.press('home');
      await app.keyboard.press('enter');
      await app.waitFor('text:8 集 · 384.0 MB · 剩余 32.0 GB');
      expect(app.value('id:merge-output')).toBe('装穷提亲，娶到全家嫌弃的她');
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/merge-' + colorScheme + '-ready-1280x840.png');
      await app.press('role:combobox[选择剧集]');
      await app.keyboard.press('end');
      await app.keyboard.press('enter');
      expect(app.value('id:merge-output')).toBe('下一段故事');
      expect(app.enabled('role:tab[快速合并]')).toBe(false);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/merge-' + colorScheme + '-compat-1280x840.png');
      await app.press('role:button[开始合并]');
      await app.waitFor('text:开始合并');
      expect(app.enabled('role:button[合并中…]')).toBe(false);
    },
    60_000,
  );

  test(
    'maintenance storage page ' + colorScheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        platform: 'macos',
        colorScheme,
        locale: 'zh-CN',
      });
      await app.press('id:main-nav >> role:button[磁盘清理]');
      await app.waitFor('text:768.0 MB');
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/storage-' + colorScheme + '-expanded-1280x840.png');
      await app.type('id:storage-search', '下一段');
      expect(app.shows('id:storage-rows >> text:下一段故事')).toBe(true);
      expect(app.shows('id:storage-rows >> text:归途有风')).toBe(false);
      await app.press('role:button[删除《下一段故事》的本地文件]');
      await app.waitFor('text:576.0 MB');
      app.onDialog(() => '取消');
      await app.press('role:button[删除全部已下载]');
      expect(app.shows('text:576.0 MB')).toBe(true);
      app.onDialog(() => '确定');
      await app.press('role:button[删除全部已下载]');
      await app.waitFor('text:0 B');
    },
    60_000,
  );
}
