import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('rich editor atomic emoji deletion and wire submission ' + scheme, async () => {
    const app = await open('.', {
      width: 1280,
      height: 840,
      colorScheme: scheme,
      locale: 'zh-CN',
    });
    await app.waitFor('text:00:42 / 02:49');
    await app.keyboard.press('mod+shift+e');
    await app.waitFor('离线表情与附件验证');
    await app.type('id:emoji-fixture >> label:留下你的精彩评论吧', 'A');
    await app.keyboard.press('end');
    await app.keyboard.press('backspace');
    await app.keyboard.press('backspace');
    await app.press('id:emoji-fixture >> role:button[表情]');
    await app.waitFor('role:button[[偷笑]]');
    await app.press('role:button[[偷笑]]');
    await app.keyboard.press('escape');
    await app.waitFor('id:emoji-fixture >> label:[偷笑]');
    await app.screenshot('target/nsh-preview/emoji-editor-' + scheme + '.png');
    await app.keyboard.press('enter');
    await app.waitFor('已发送：你好[偷笑]');
    expect(app.shows('id:emoji-fixture >> label:[偷笑]')).toBe(false);
    const longDraft = '中文[偷笑]'.repeat(20);
    await app.type('id:emoji-fixture >> label:留下你的精彩评论吧', longDraft);
    await app.keyboard.press('end');
    await app.screenshot('target/nsh-preview/emoji-editor-long-end-' + scheme + '.png');
    await app.keyboard.press('home');
    await app.screenshot('target/nsh-preview/emoji-editor-long-home-' + scheme + '.png');
    await app.keyboard.press('enter');
    await app.waitFor('已发送：' + longDraft);
  });
}
