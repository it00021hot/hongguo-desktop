import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test('emoji selection search collection and attachments ' + scheme, async () => {
    const app = await open('.', { width: 1280, height: 840, colorScheme: scheme, locale: 'zh-CN' });
    await app.waitFor('text:00:42 / 02:49');
    await app.keyboard.press('mod+shift+e');
    await app.waitFor('离线表情与附件验证');
    await app.press('id:emoji-fixture >> role:button[表情]');
    await app.waitFor('role:button[[偷笑]]');
    await app.press('role:button[[偷笑]]');
    expect(app.shows('role:button[[飞吻]]')).toBe(true);
    await app.screenshot('target/nsh-preview/emoji-system-' + scheme + '.png');
    await app.press('role:button[搜索表情]');
    await app.type('搜索表情包…', '笑');
    await app.waitFor('role:button[测试表情0]');
    await app.rightClick('role:button[测试表情0]');
    await app.waitFor('添加到表情');
    await app.press('添加到表情');
    expect(app.shows('role:menuitem[添加到表情]')).toBe(false);
    await app.press('role:button[自定义表情]');
    await app.waitFor('role:button[上传自定义表情]');
    await app.waitFor('role:button[测试表情0]');
    await app.press('role:button[测试表情0]');
    await app.keyboard.press('escape');
    await app.waitFor('role:button[移除图片]');
    await app.press('role:button[模拟发送失败]');
    await app.press('id:emoji-fixture >> role:button[发送]');
    await app.waitFor('离线模拟：发送失败，草稿已保留');
    expect(app.shows('role:button[移除图片]')).toBe(true);
    await app.screenshot('target/nsh-preview/emoji-attachment-' + scheme + '.png');
    await app.press('id:emoji-fixture >> role:button[发送]');
    await app.waitFor('已发送：你好[微笑][偷笑]（含图片）');
    await app.press('id:emoji-fixture >> role:button[图片]');
    await app.press('id:emoji-fixture >> role:button[发送]');
    await app.waitFor('已发送：（含图片）');
  });
}
