import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

for (const width of [1280, 1100]) {
  test('desktop compact danmaku popup accepts and preserves mixed text at ' + width, async () => {
    const app = await open('.', {
      width,
      height: 840,
      platform: 'macos',
      colorScheme: 'dark',
      locale: 'zh-CN',
    });
    const input = 'label:发条友善的弹幕吧';
    const trigger = 'role:button[发送弹幕]';
    const popup = 'id:player-danmaku-send-popup';
    const draft = '中文 Abc123';
    await app.waitFor(trigger);
    expect(app.shows(popup)).toBe(false);
    await app.screenshot('target/nsh-preview/danmaku-input-before-' + width + '.png');
    await app.press(trigger);
    await app.waitFor(popup);
    await app.waitFor(input);
    expect(app.enabled(input)).toBe(true);
    // type focuses the labelled native input; RichEditor's label itself is not a Button.
    await app.type(input, draft);
    await app.waitFor('text:' + draft);
    expect(app.enabled('role:button[发送]')).toBe(true);
    await app.screenshot('target/nsh-preview/danmaku-input-typed-' + width + '.png');
    // This fixture uses the production composer and a local no-op submit handler.
    // Exercise both submission entrances without logging in or sending an API request.
    await app.keyboard.press('enter');
    await app.waitFor('text:' + draft);
    await app.press('role:button[发送]');
    await app.waitFor('text:' + draft);
    await app.keyboard.press('escape');
    await app.waitFor(popup, { state: 'gone' });
    // Space reopens only if closing the popup returned focus to its real trigger.
    await app.keyboard.press('space');
    await app.waitFor(popup);
    await app.waitFor('text:' + draft);
    await app.screenshot('target/nsh-preview/danmaku-input-reopened-' + width + '.png');
    await app.keyboard.press('escape');
    await app.waitFor(popup, { state: 'gone' });
    expect(app.requests()).toEqual([]);
  });
}
