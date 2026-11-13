import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

test('view memory finishing rank fresh narrow dark', async () => {
  const app = await open('.', { width: 393, height: 840, colorScheme: 'dark', locale: 'zh-CN' });
  await app.press('id:bottom-nav >> role:button[排行榜]');
  await app.waitFor('id:rank-header');
  await app.keyboard.press('mod+shift+u');
  await app.waitFor('id:rank-header >> text:离线子榜 01');
  await app.screenshot('target/nsh-followup-20261113/rank-sublists-fresh-narrow-before.png');
  await app.scroll('id:rank-sub-lists', 5000);
  await app.screenshot('target/nsh-followup-20261113/rank-sublists-fresh-narrow-after.png');
  await app.press('id:rank-sub-lists >> role:button[离线子榜 24]');
  await app.waitFor('id:rank-header >> text:离线子榜 24');
});

for (const scheme of ['dark', 'light'] as const) {
  test(
    'view memory finishing rank sublist scroll axes and selection ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('id:main-nav');
      await app.press('id:main-nav >> role:button[排行榜]');
      await app.waitFor('id:rank-header');
      await app.keyboard.press('mod+shift+u');
      const header = 'id:rank-header >> text:';
      const list = 'id:rank-sub-lists';
      const first = list + ' >> role:button[离线子榜 01]';
      const last = list + ' >> role:button[离线子榜 24]';
      await app.waitFor(header + '离线子榜 01');
      expect(app.count(list + ' >> role:button')).toBe(24);
      await app.hover(first);
      await app.screenshot(
        'target/nsh-followup-20261113/rank-sublists-desktop-first-' + scheme + '.png',
      );
      await app.scroll(list, 5000);
      await app.press(last);
      await app.waitFor(header + '离线子榜 24');
      const selected = app.spanStyle(last + ' >> text:离线子榜 24').color;
      await app.hover(last);
      await app.screenshot(
        'target/nsh-followup-20261113/rank-sublists-desktop-last-' + scheme + '.png',
      );
      await app.press('id:rank-header >> role:button[月榜]');
      await app.waitFor('id:rank-month-filter');
      await app.keyboard.press('escape');
      await app.waitFor('id:rank-month-filter', { state: 'gone' });
      await app.wait(150);
      await app.keyboard.press('shift+tab');
      await app.keyboard.press('home');
      await app.keyboard.press('right');
      await app.waitFor(header + '离线子榜 24');
      await app.keyboard.press('enter');
      await app.waitFor(header + '离线子榜 02');
      await app.screenshot(
        'target/nsh-followup-20261113/rank-sublists-desktop-focus-' + scheme + '.png',
      );
      await app.scroll(list, 5000);
      await app.press(last);
      await app.setViewport(393, 840);
      await app.wait(250);
      await app.screenshot(
        'target/nsh-followup-20261113/rank-sublists-narrow-before-' + scheme + '.png',
      );
      await app.scroll(list, 5000);
      await app.screenshot(
        'target/nsh-followup-20261113/rank-sublists-narrow-after-' + scheme + '.png',
      );
      await app.press(last);
      await app.waitFor(header + '离线子榜 24');
      expect(app.spanStyle(last + ' >> text:离线子榜 24').color).toBe(selected);
      await app.hover(last);
      await app.screenshot(
        'target/nsh-followup-20261113/rank-sublists-narrow-last-' + scheme + '.png',
      );
      await app.scroll(list, -5000);
      await app.press(first);
      await app.waitFor(header + '离线子榜 01');
      await app.hover(first);
      await app.screenshot(
        'target/nsh-followup-20261113/rank-sublists-narrow-first-' + scheme + '.png',
      );
      await app.setViewport(1280, 840);
      await app.wait(250);
      await app.waitFor(header + '离线子榜 01');
      await app.scroll(list, 5000);
      await app.press(last);
      await app.waitFor(header + '离线子榜 24');
      if (scheme === 'dark') {
        await app.keyboard.press('mod+shift+u');
        await app.waitFor(header + '热播榜');
        await app.scroll('id:rank-results', 400);
        await app.screenshot('target/nsh-followup-20261113/rank-main-scroll-before-dark.png');
        await app.back();
        await app.waitFor('id:rank-results', { state: 'gone' });
        await app.press('id:main-nav >> role:button[排行榜]');
        await app.waitFor(header + '热播榜');
        await app.waitFor('id:rank-results');
        await app.screenshot('target/nsh-followup-20261113/rank-main-scroll-return-dark.png');
        await app.press('id:rank-results >> role:button[最好的时光]');
        await app.waitFor('id:detail-scroll >> text:最好的时光');
      }
    },
    60_000,
  );
}
