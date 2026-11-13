import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('browse page filters and search ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      platform: 'macos',
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.press('id:main-nav >> role:button[找剧]');
    await app.waitFor('id:browse-results');
    await app.hover('id:app-toolbar-rest');
    await app.wait(200);
    await app.screenshot('target/nsh-preview/browse-' + scheme + '-expanded-1280x840.png');
    await app.press('role:button[逆袭]');
    await app.press('role:button[逆袭]');
    await app.press('role:button[真人剧]');
    await app.press('role:button[逆袭]');
    await app.press('role:button[总裁]');
    await app.press('role:button[现代]');
    await app.press('role:button[最近7天]');
    expect(app.shows('role:button[筛选] >> text:5')).toBe(true);
    await app.hover('id:app-toolbar-rest');
    await app.wait(200);
    await app.screenshot('target/nsh-preview/browse-filters-' + scheme + '-expanded-1280x840.png');
    await app.press('role:button[筛选]');
    expect(app.shows('text:长度')).toBe(false);
    await app.press('role:button[筛选]');
    await app.type('id:browse-query', '装穷');
    await app.waitFor('id:browse-suggestions');
    await app.keyboard.press('down');
    await app.keyboard.press('enter');
    await app.waitFor('role:button[退出搜索]');
    expect(app.shows('role:button[筛选]')).toBe(false);
    expect(app.shows('text:共 10 部')).toBe(false);
    await app.hover('id:app-toolbar-rest');
    await app.wait(200);
    await app.screenshot('target/nsh-preview/browse-search-' + scheme + '-expanded-1280x840.png');
    await app.press('role:button[退出搜索]');
    expect(app.shows('role:button[筛选] >> text:5')).toBe(true);
  });
}
