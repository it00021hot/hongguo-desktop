import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;
const form = 'id:preferences-form >> id:form-scroll';

test('production settings registers and removes its desktop global shortcut', async () => {
  const app = await open('.', {
    width: 1280,
    height: 840,
    platform: 'macos',
    colorScheme: 'dark',
    locale: 'zh-CN',
    network: 'live',
  });
  await app.waitFor('id:main-nav >> role:button[设置]', { timeout: 30_000 });
  await app.press('id:main-nav >> role:button[设置]');
  await app.waitFor('role:combobox[文件命名模板]');
  const revealBoss = async () => {
    await app.scroll(form, 5000);
    await app.scroll(form, -450);
  };
  const save = async () => {
    await app.scroll(form, 5000);
    await app.press('role:button[保存]');
    await app.waitFor('text:设置已保存', { timeout: 20_000 });
  };
  let registered = false;
  try {
    await revealBoss();
    await app.press('role:button[老板键]');
    await app.keyboard.press('mod+alt+shift+k');
    await app.waitFor('text:Ctrl/Cmd + Alt + Shift + K');
    await save();
    registered = true;
    await revealBoss();
    await app.press('role:button[老板键]');
    await app.keyboard.press('backspace');
    await app.waitFor('text:关闭');
    await save();
    registered = false;
    await app.hover('id:app-toolbar-rest');
    await app.screenshot('target/nsh-playback/settings-native-registration.png');
    expect(app.shows('text:设置已保存')).toBe(true);
  } finally {
    if (registered) {
      await revealBoss();
      await app.press('role:button[老板键]');
      await app.keyboard.press('backspace');
      await save();
    }
  }
}, 90_000);
