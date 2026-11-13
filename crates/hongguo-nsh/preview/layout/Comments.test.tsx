import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'desktop corrections comments pagination matches compact centered Tauri button ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('id:app-toolbar-rest');
      await app.press('role:button[倍速]');
      await app.waitFor('text:1.5x');
      await app.keyboard.press('escape');
      await app.keyboard.press('mod+shift+c');
      await app.waitFor('id:player-comments-panel');
      await app.scroll('id:comments-list', 5000);
      await app.scroll('id:comments-list', 5000);
      await app.waitFor('role:button[加载更多评论（还有 20 条）]');
      await app.screenshot('target/nsh-preview/comments-pagination-' + scheme + '.png');
      await app.press('role:button[加载更多评论（还有 20 条）]');
      expect(app.shows('role:button[加载更多评论（还有 20 条）]')).toBe(false);
    },
  );

  test('comments finishing desktop interaction and editor ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('id:app-toolbar-rest');
    await app.press('role:button[倍速]');
    await app.waitFor('text:1.5x');
    await app.keyboard.press('escape');
    await app.keyboard.press('mod+shift+c');
    await app.waitFor('id:player-comments-panel');
    await app.waitFor('回复的观众');
    await app.screenshot('target/nsh-preview/comments-' + scheme + '-initial.png');
    await app.press('id:comment-review-preview-0 >> role:button[点赞 12]');
    await app.waitFor('id:comment-review-preview-0 >> role:button[已点赞 13]');
    await app.press('id:comment-review-preview-0 >> role:button[收起回复]');
    expect(app.shows('id:comment-review-preview-0-reply')).toBe(false);
    await app.press('id:comment-review-preview-0 >> role:button[展开 1 条回复]');
    await app.waitFor('id:comment-review-preview-0-reply');
    await app.press('id:comment-review-preview-0 >> role:button[回复]');
    await app.type('id:comment-review-preview-0 >> label:留下你的精彩评论吧', '原地回复[微笑]');
    await app.press('id:comment-review-preview-0 >> role:button[发送]');
    await app.waitFor('id:comment-review-preview-0-sent-1');
    await app.press('id:comment-review-preview-0 >> role:button[收起回复]');
    await app.waitFor('id:comment-review-preview-0 >> role:button[展开 2 条回复]');
    await app.press('id:comment-review-preview-0 >> role:button[展开 2 条回复]');
    await app.waitFor('id:comment-review-preview-0-sent-1');
    await app.screenshot('target/nsh-preview/comments-' + scheme + '-expanded.png');
    await app.press('id:player-comments-panel >> role:button[图片]');
    await app.waitFor('id:player-comments-panel >> role:button[移除图片]');
    await app.screenshot('target/nsh-preview/comments-' + scheme + '-attachment.png');
    await app.press('id:player-comments-panel >> role:button[移除图片]');
    await app.keyboard.press('mod+shift+0');
    await app.waitFor('还没有评论，来抢沙发');
    await app.screenshot('target/nsh-preview/comments-' + scheme + '-empty.png');
    await app.keyboard.press('escape');
    expect(app.shows('id:player-comments-panel')).toBe(false);
    await app.keyboard.press('c');
    await app.waitFor('id:player-comments-panel');
  });
}
