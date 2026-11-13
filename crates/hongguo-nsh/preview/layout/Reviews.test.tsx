import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'comments finishing details reviews share the production virtual list and editor ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:@装穷提亲，娶到全家嫌弃的她');
      await app.press('text:@装穷提亲，娶到全家嫌弃的她');
      await app.waitFor('id:detail-scroll >> text:第1集');
      await app.press('text:剧评');
      await app.waitFor('id:reviews-list');
      await app.scroll('id:reviews-list', 600);
      await app.waitFor('第1位观众');
      await app.waitFor('回复的观众');
      await app.hover('id:app-toolbar-rest');
      await app.screenshot('target/nsh-preview/reviews-' + scheme + '-expanded-1280x840.png');
      await app.scroll('id:reviews-list', -600);
      await app.type('写下你的剧评…', '这是一条完整的剧评[微笑]');
      await app.press('role:radio[评 2 星]');
      await app.keyboard.press('right');
      await app.press('role:button[发表剧评]');
      await app.waitFor('id:comment-sent-100');
      await app.waitFor('id:comment-sent-100 >> role:image[评 3 星]');
      await app.scroll('id:reviews-list', 600);
      await app.waitFor('刚刚发表');
      await app.screenshot('target/nsh-preview/reviews-' + scheme + '-sent.png');
      await app.scroll('id:reviews-list', 1500);
      expect(app.shows('id:comment-sent-100')).toBe(false);
      await app.screenshot('target/nsh-preview/reviews-' + scheme + '-scrolled.png');
      await app.scroll('id:reviews-list', -2100);
      await app.waitFor('id:comment-sent-100');
      await app.keyboard.press('mod+shift+0');
      await app.waitFor('text:这一集还没有评论');
      await app.screenshot('target/nsh-preview/reviews-' + scheme + '-empty.png');
    },
  );
}
