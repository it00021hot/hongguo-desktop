import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  for (const page of ['browse', 'rank'] as const) {
    test(page + ' production presenter ' + scheme, async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.press('role:button[' + (page === 'browse' ? '找剧' : '排行榜') + ']');
      await app.waitFor('id:' + page + '-results');
      await app.hover('id:app-toolbar-rest');
      await app.wait(250);
      await app.screenshot('target/nsh-preview/' + page + '-' + scheme + '-expanded-1280x840.png');
      if (page === 'browse') {
        expect(app.shows('text:长度')).toBe(true);
        await app.press('role:button[筛选]');
        expect(app.shows('text:长度')).toBe(false);
        await app.press('role:button[筛选]');
        expect(app.shows('text:长度')).toBe(true);
      } else {
        await app.press('role:button[推荐榜]');
        expect(app.shows('id:rank-header >> text:推荐榜')).toBe(true);
      }
    });
  }
}
