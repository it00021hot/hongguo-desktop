import { expect, open, test as registerTest } from 'nsh/test';
const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;
for (const colorScheme of ['dark', 'light'] as const) {
  test(
    'upcoming source snapshot and reserve ' + colorScheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        platform: 'macos',
        colorScheme,
        locale: 'zh-CN',
      });
      await app.press('text:@装穷提亲，娶到全家嫌弃的她');
      await app.press('role:button[看「离线创作者」的主页]');
      await app.waitFor('text:作品 · 10');
      await app.press('role:button[下一段故事]');
      await app.waitFor('id:detail-scroll >> role:button[预约]');
      expect(app.shows('text:即将上线')).toBe(true);
      expect(app.shows('role:button[从第 1 集开始]')).toBe(false);
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/upcoming-' + colorScheme + '-expanded-1280x840.png');
      await app.press('id:detail-scroll >> role:button[预约]');
      await app.waitFor('id:detail-scroll >> role:button[已预约]');
      await app.press('id:detail-scroll >> role:button[已预约]');
      await app.waitFor('id:detail-scroll >> role:button[预约]');
    },
    60_000,
  );
  test(
    'narrow detail actions stay above bottom nav ' + colorScheme,
    async () => {
      const app = await open('.', {
        width: 480,
        height: 840,
        platform: 'macos',
        colorScheme,
        locale: 'zh-CN',
      });
      await app.press('text:@装穷提亲，娶到全家嫌弃的她');
      await app.waitFor('id:detail-mobile-actions');
      await app.waitFor('id:bottom-nav');
      await app.hover('id:app-toolbar-rest');
      await app.wait(200);
      await app.screenshot('target/nsh-preview/detail-' + colorScheme + '-expanded-480x840.png');
      await app.press('id:detail-mobile-actions >> role:button[收藏]');
      await app.waitFor('id:detail-mobile-actions >> role:button[已收藏]');
      const before = app.spanStyle('id:detail-mobile-actions >> text:点赞').color;
      await app.press('id:detail-mobile-actions >> role:button[点赞]');
      expect(app.spanStyle('id:detail-mobile-actions >> text:点赞').color === before).toBe(false);
    },
    60_000,
  );
}
