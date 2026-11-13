import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('library empty matches production source ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('text:00:42 / 02:49');
    await app.press('id:main-nav >> role:button[历史]');
    await app.waitFor('text:看到第1集/共80集 00:42');
    await app.press('role:button[管理]');
    await app.press('role:button[全选]');
    await app.press('role:button[删除所选]');
    await app.press('id:confirmation >> role:button[确定]');
    await app.waitFor('id:history-empty');
    await app.waitFor('text:在官方客户端或本应用看过的短剧都会同步到这里');
    expect(app.shows('role:button[管理]')).toBe(false);
    expect(app.shows('text:未登录：观看记录保存在本机，登录后自动同步到云端')).toBe(false);
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-preview/history-' + scheme + '-empty-1280x840.png');
  });
}
