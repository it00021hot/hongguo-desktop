import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;
for (const scheme of ['dark', 'light'] as const) {
  test('tauri alignment: rank independent anchored filters ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.press('role:button[排行榜]');
    await app.waitFor('id:rank-results');
    await app.press('id:rank-header >> role:button[月榜]');
    await app.waitFor('id:rank-month-filter');
    expect(app.shows('id:rank-total-filter')).toBe(false);
    await app.screenshot('target/nsh-preview/rank-month-' + scheme + '.png');
    await app.press('id:rank-month-filter >> role:button[2026年10月]');
    expect(app.shows('id:rank-month-filter')).toBe(false);
    await app.press('id:rank-header >> role:button[总榜]');
    await app.waitFor('id:rank-total-filter');
    expect(app.shows('id:rank-month-filter')).toBe(false);
    await app.screenshot('target/nsh-preview/rank-total-' + scheme + '.png');
    await app.press('id:rank-total-filter >> role:button[恋爱]');
    expect(app.shows('id:rank-total-filter')).toBe(false);
    expect(app.shows('id:rank-header >> role:button[2026年10月]')).toBe(true);
    expect(app.shows('id:rank-header >> role:button[恋爱]')).toBe(true);
    await app.press('id:rank-header >> role:button[恋爱]');
    await app.waitFor('id:rank-total-filter');
    await app.keyboard.press('escape');
    expect(app.shows('id:rank-total-filter')).toBe(false);
  });
}
