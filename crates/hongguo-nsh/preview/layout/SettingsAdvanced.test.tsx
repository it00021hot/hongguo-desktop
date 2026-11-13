import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'settings final advanced ' + scheme,
    async () => {
      const app = await open('.', {
        width: 1280,
        height: 840,
        colorScheme: scheme,
        locale: 'zh-CN',
      });
      await app.waitFor('text:00:42 / 02:49');
      await app.press('id:main-nav >> role:button[设置]');
      await app.waitFor('role:combobox[文件命名模板]');
      await app.hover('id:app-toolbar-rest');
      await app.screenshot('target/nsh-preview/settings-' + scheme + '-expanded-1280x840.png');
      await app.scroll('id:preferences-form >> id:form-scroll', 757);
      await app.hover('id:app-toolbar-rest');
      await app.wait(160);
      await app.screenshot('target/nsh-preview/settings-' + scheme + '-playback-1280x840.png');
      await app.scroll('id:preferences-form >> id:form-scroll', 5000);
      await app.hover('id:app-toolbar-rest');
      await app.wait(160);
      await app.screenshot('target/nsh-preview/settings-' + scheme + '-bottom-1280x840.png');
      await app.press('role:combobox[保留周期]');
      await app.keyboard.press('end');
      await app.keyboard.press('enter');
      await app.waitFor('text:30 天');
      await app.press('role:button[清理缓存]');
      await app.waitFor('text:已清理 0 项，释放 0 B');
      await app.scroll('id:preferences-form >> id:form-scroll', -450);
      await app.press('role:button[播放 / 暂停]');
      await app.keyboard.press('j');
      await app.waitFor('text:J');
      await app.press('role:button[老板键]');
      await app.keyboard.press('mod+alt+j');
      await app.waitFor('text:Ctrl/Cmd + Alt + J');
      await app.scroll('id:preferences-form >> id:form-scroll', 5000);
      await app.press('role:button[保存]');
      await app.waitFor('text:老板键与播放器快捷键冲突，请先调整老板键或播放器快捷键再保存');
      expect(app.shows('text:设置已保存')).toBe(false);
      await app.scroll('id:preferences-form >> id:form-scroll', -450);
      await app.press('role:button[老板键]');
      await app.keyboard.press('backspace');
      await app.waitFor('role:button[老板键] >> text:关闭');
      await app.scroll('id:preferences-form >> id:form-scroll', 5000);
      await app.press('role:button[保存]');
      await app.waitFor('text:设置已保存');
    },
    60_000,
  );
}
