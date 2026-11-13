import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const colorScheme of ['dark', 'light'] as const) {
  test(
    'creator profile and real work navigation ' + colorScheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        platform: 'macos',
        colorScheme,
        locale: 'zh-CN',
      });
      await app.press('text:@装穷提亲，娶到全家嫌弃的她');
      await app.waitFor('role:button[看「离线创作者」的主页]');
      expect(app.enabled('role:button[看「幕后团队」的主页]')).toBe(false);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot(
        'target/nsh-preview/detail-creators-' + colorScheme + '-expanded-1280x840.png',
      );
      await app.press('role:button[看「离线创作者」的主页]');
      await app.waitFor('text:作品 · 10');
      expect(app.shows('text:认真讲好每一个故事，用作品与观众相遇。')).toBe(true);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/creator-' + colorScheme + '-expanded-1280x840.png');
      await app.press('role:button[装穷提亲，娶到全家嫌弃的她]');
      await app.waitFor('text:剧情简介');
      expect(app.shows('text:装穷提亲，娶到全家嫌弃的她')).toBe(true);
    },
    60_000,
  );
}
