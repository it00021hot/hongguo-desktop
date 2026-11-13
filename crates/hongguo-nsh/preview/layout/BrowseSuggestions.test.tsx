import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('browse suggestions keep input focus and choose words ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      platform: 'macos',
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.press('id:main-nav >> role:button[找剧]');
    await app.waitFor('id:browse-query');
    await app.type('id:browse-query', '装穷');
    await app.waitFor('id:browse-suggestions >> role:option[装穷提亲，娶到全家嫌弃的她]');
    await app.hover('id:app-toolbar-rest');
    await app.wait(200);
    await app.screenshot(
      'target/nsh-preview/browse-suggestions-' + scheme + '-expanded-1280x840.png',
    );
    await app.keyboard.press('down');
    await app.keyboard.press('enter');
    await app.waitFor('role:button[退出搜索]');
    expect(app.shows('id:browse-suggestions')).toBe(false);
    await app.press('role:button[退出搜索]');
    await app.type('id:browse-query', '装穷');
    expect(app.value('id:browse-query')).toBe('装穷');
    await app.waitFor('id:browse-suggestions');
    await app.press('id:main-nav >> role:button[找剧]');
    expect(app.shows('id:browse-suggestions')).toBe(false);
    await app.keyboard.press('end');
    for (
      let remaining = Array.from(app.value('id:browse-query') ?? '').length;
      remaining > 0;
      remaining--
    ) {
      await app.keyboard.press('backspace');
    }
    expect(app.value('id:browse-query')).toBe('');
    await app.type('id:browse-query', '装穷');
    expect(app.value('id:browse-query')).toBe('装穷');
    await app.waitFor('id:browse-suggestions');
    await app.keyboard.press('escape');
    expect(app.shows('id:browse-suggestions')).toBe(false);
  });
}
