import { type App, expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

async function closed(app: App, selector: string) {
  await app.waitUntil(() => !app.shows(selector));
}

for (const scheme of ['dark', 'light'] as const) {
  test('menu audit pending long rank filters scroll to real final options ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      platform: 'macos',
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.press('id:main-nav >> role:button[排行榜]');
    await app.waitFor('id:rank-header');
    await app.keyboard.press('mod+shift+r');
    await app.press('id:rank-header >> role:button[总榜]');
    await app.waitFor('id:rank-total-filter');
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-preview/menu-audit-long-total-before-' + scheme + '.png');
    await app.scroll('id:rank-total-filter', 5000);
    await app.screenshot('target/nsh-preview/menu-audit-long-total-after-' + scheme + '.png');
    await app.press('id:rank-total-filter >> role:button[长列表末项]');
    await app.waitFor('id:rank-header >> role:button[长列表末项]');
    await closed(app, 'id:rank-total-filter');
    await app.press('id:rank-header >> role:button[月榜]');
    await app.waitFor('id:rank-month-filter');
    await app.screenshot('target/nsh-preview/menu-audit-long-month-before-' + scheme + '.png');
    await app.scroll('id:rank-month-filter', 5000);
    await app.screenshot('target/nsh-preview/menu-audit-long-month-after-' + scheme + '.png');
    await app.press('id:rank-month-filter >> role:button[2026年12月]');
    await app.waitFor('id:rank-header >> role:button[2026年12月]');
    await closed(app, 'id:rank-month-filter');
  });
  test(
    (scheme === 'dark' ? 'menu audit finishing' : 'menu audit verified') +
      ' global language theme and settings focus ' +
      scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        platform: 'macos',
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.press('role:button[语言]');
      await app.waitFor('role:menuitem[English]');
      await app.keyboard.press('escape');
      await closed(app, 'role:menuitem[English]');
      await app.keyboard.press('enter');
      await app.waitFor('role:menuitem[English]');
      await app.press('role:menuitem[English]');
      await app.waitFor('role:button[Language]');
      await closed(app, 'role:menuitem[English]');
      await app.press('role:button[Language]');
      await app.waitFor('role:menuitem[Simplified Chinese]');
      await app.press('role:menuitem[Simplified Chinese]');
      await app.waitFor('role:button[语言]');
      const previous = app.spanStyle('id:main-nav >> text:设置').color;
      await app.press('role:button[切换主题]');
      await app.waitFor('role:menuitem[跟随系统]');
      await app.press('role:menuitem[' + (scheme === 'dark' ? '亮色' : '暗色') + ']');
      await app.waitUntil(() => app.spanStyle('id:main-nav >> text:设置').color !== previous);
      await closed(app, 'role:menuitem[跟随系统]');
      await app.press('id:main-nav >> role:button[设置]');
      await app.waitFor('role:combobox[文件命名模板]');
      const naming = 'role:combobox[文件命名模板]';
      await app.press(naming);
      await app.keyboard.press('end');
      await app.keyboard.press('escape');
      await closed(app, 'text:仅剧名');
      expect(app.shows(naming + ' >> text:剧名 集数')).toBe(true);
      await app.keyboard.press('enter');
      await app.waitFor('text:仅剧名');
      await app.keyboard.press('end');
      await app.keyboard.press('enter');
      await app.waitFor(naming + ' >> text:仅剧名');
      await app.press(naming);
      expect(app.count('text:仅剧名')).toBe(2);
      await app.press(naming);
      await app.waitUntil(() => app.count('text:仅剧名') === 1);
      await app.hover('id:app-toolbar-rest');
      await app.screenshot('target/nsh-preview/menu-audit-settings-' + scheme + '.png');
      if (scheme === 'dark') {
        await app.press('id:main-nav >> role:button[一键合并]');
        await app.waitFor('id:merge-series');
        await app.press('role:combobox[选择剧集]');
        await app.keyboard.press('home');
        await app.keyboard.press('enter');
        await app.waitUntil(() => app.value('id:merge-output') === '装穷提亲，娶到全家嫌弃的她');
        await app.press('role:combobox[选择剧集]');
        await app.keyboard.press('end');
        await app.keyboard.press('enter');
        await app.waitUntil(() => app.value('id:merge-output') === '下一段故事');
      }
    },
    60_000,
  );

  test(
    'menu audit rank filters close select and survive route changes ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        platform: 'macos',
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.press('id:main-nav >> role:button[排行榜]');
      await app.waitFor('id:rank-header');
      await app.press('id:rank-header >> role:button[总榜]');
      await app.waitFor('id:rank-total-filter');
      await app.keyboard.press('escape');
      await closed(app, 'id:rank-total-filter');
      await app.keyboard.press('enter');
      await app.waitFor('id:rank-total-filter');
      await app.press('id:rank-total-filter >> role:button[恋爱]');
      await app.waitFor('id:rank-header >> role:button[恋爱]');
      await closed(app, 'id:rank-total-filter');
      await app.press('id:rank-header >> role:button[月榜]');
      await app.waitFor('id:rank-month-filter');
      await app.press('id:rank-month-filter >> role:button[2026年10月]');
      await app.waitFor('id:rank-header >> role:button[2026年10月]');
      await app.press('id:rank-header >> role:button[恋爱]');
      await app.waitFor('id:rank-total-filter');
      await app.press('id:main-nav >> role:button[新剧]');
      await app.waitFor('role:button[上新日历]');
      await closed(app, 'id:rank-total-filter');
      await app.press('id:main-nav >> role:button[排行榜]');
      await app.waitFor('id:rank-header >> role:button[恋爱]');
      await closed(app, 'id:rank-total-filter');
      await app.hover('id:app-toolbar-rest');
      await app.screenshot('target/nsh-preview/menu-audit-rank-' + scheme + '.png');
    },
    60_000,
  );

  test(
    'menu audit pending browse suggestions and new drama dates retain their own input ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        platform: 'macos',
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.press('id:main-nav >> role:button[找剧]');
      await app.type('id:browse-query', '装穷');
      await app.waitFor('id:browse-suggestions');
      await app.keyboard.press('escape');
      expect(app.value('id:browse-query')).toBe('装穷');
      await closed(app, 'id:browse-suggestions');
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
      await app.keyboard.press('down');
      await app.keyboard.press('enter');
      await app.waitFor('role:button[退出搜索]');
      await app.press('id:main-nav >> role:button[新剧]');
      await app.press('role:button[上新日历]');
      await app.waitFor('id:new-calendar-list');
      await app.press('role:radio[周四 11/12]');
      await app.waitFor('text:这一天没有上新');
      await app.press('role:radio[今天 11/11]');
      await app.waitFor('id:new-calendar-list >> text:归途有风');
      await app.press('role:button[新剧推荐]');
      const beforeChannel = app.spanStyle('role:button[女频] >> text:女频').color;
      await app.press('role:button[女频]');
      await app.waitUntil(
        () => app.spanStyle('role:button[女频] >> text:女频').color !== beforeChannel,
      );
      await closed(app, 'id:browse-suggestions');
      await app.hover('id:app-toolbar-rest');
      await app.screenshot('target/nsh-preview/menu-audit-new-' + scheme + '.png');
    },
    60_000,
  );
}
