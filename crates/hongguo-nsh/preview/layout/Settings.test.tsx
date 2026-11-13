import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const colorScheme of ['dark', 'light'] as const) {
  test(`settings layout ${colorScheme} follows the production form`, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme, locale: 'zh-CN' });
    await app.press('id:main-nav >> role:button[设置]');
    await app.waitFor('text:登录后可预约新剧、同步观看记录');
    expect(app.shows('role:combobox[文件命名模板]')).toBe(true);
    expect(app.shows('role:adjustable[最大并发数]')).toBe(true);
    await app.hover('id:app-toolbar-rest');
    await app.screenshot(`target/nsh-preview/settings-${colorScheme}-expanded-1280x840.png`);
    if (colorScheme === 'dark') {
      await app.press('role:combobox[文件命名模板]');
      await app.keyboard.press('end');
      await app.keyboard.press('enter');
      await app.waitFor('text:仅剧名');
      await app.keyboard.press('tab');
      await app.keyboard.press('right');
      await app.waitFor('text:最大并发数 — 4');
    }
  });
}
