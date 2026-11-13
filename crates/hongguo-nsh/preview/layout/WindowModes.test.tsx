import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  for (const mode of ['mini', 'portrait'] as const) {
    test(
      'window transport shares state and waits for acknowledgement ' + mode + ' ' + scheme,
      async () => {
        const app = await open('.', {
          width: mode === 'mini' ? 480 : 360,
          height: mode === 'mini' ? 270 : 780,
          platform: 'macos',
          colorScheme: scheme,
          locale: 'zh-CN',
        });
        await app.waitFor('id:player-stage');
        await app.keyboard.press('mod+shift+w');
        await app.waitFor('id:window-preview-state');
        await app.keyboard.press(mode === 'mini' ? 'mod+alt+m' : 'mod+alt+p');
        await app.waitFor(
          'text:window:Normal pending:true fill:false frame:false pinned:false error:',
        );
        await app.keyboard.press('mod+alt+a');
        await app.waitFor('id:player-window-controls');
        await app.waitFor(
          'text:window:' +
            (mode === 'mini' ? 'Mini' : 'Portrait') +
            ' pending:false fill:false frame:false pinned:false error:',
        );
        expect(app.count('id:window-fixture-stage')).toBe(1);
        expect(app.shows('id:bottom-nav')).toBe(false);
        expect(app.shows('id:app-titlebar')).toBe(mode === 'portrait');
        await app.keyboard.press('mod+alt+v');
        await app.screenshot('target/nsh-preview/window-' + mode + '-' + scheme + '.png');
        await app.press('id:player-window-controls >> role:button[播放]');
        await app.waitFor('id:player-window-controls >> role:button[暂停]');
        await app.press('id:player-window-controls >> role:button[倍速]');
        await app.waitFor('id:player-window-controls >> text:1.5x');
        if (mode === 'portrait') {
          await app.press('id:player-window-controls >> role:button[竖屏铺满屏幕高度]');
          await app.keyboard.press('mod+alt+a');
          await app.waitFor('id:player-window-controls >> role:button[退出铺满]');
        } else {
          await app.press('id:player-window-controls >> role:button[置顶]');
          await app.keyboard.press('mod+alt+a');
          await app.keyboard.press('mod+alt+v');
          await app.waitFor(
            'text:window:Mini pending:false fill:false frame:false pinned:true error:',
          );
        }
        await app.press(
          'id:player-window-controls >> role:button[' +
            (mode === 'mini' ? '退出小屏' : '退出竖屏') +
            ']',
        );
        await app.keyboard.press('mod+alt+a');
        await app.waitFor('id:app-titlebar');
        expect(app.shows('id:player-window-controls')).toBe(false);
        expect(app.count('id:window-fixture-stage')).toBe(1);
      },
      60_000,
    );
  }
}
