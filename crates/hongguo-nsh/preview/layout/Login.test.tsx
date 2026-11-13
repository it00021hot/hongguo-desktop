import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

for (const scheme of ['dark', 'light'] as const) {
  test(
    'settings final login ' + scheme,
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
      await app.press('id:main-nav >> role:button[我的]');
      await app.waitFor('id:login-dialog');
      expect(app.enabled('id:login-dialog >> role:button[登录]')).toBe(false);
      await app.hover('id:login-dialog');
      await app.wait(160);
      await app.screenshot('target/nsh-preview/settings-' + scheme + '-login-1280x840.png');
      await app.type('id:login-mobile', '13800138000');
      expect(app.value('id:login-mobile')).toBe('13800138000');
      expect(app.enabled('role:button[发送验证码]')).toBe(true);
      await app.press('role:button[发送验证码]');
      expect(app.texts().some((text) => /^\d+s 后重发$/.test(text))).toBe(true);
      await app.type('id:login-code', '1234');
      expect(app.enabled('id:login-dialog >> role:button[登录]')).toBe(true);
      await app.keyboard.press('escape');
      await app.waitFor('id:main-nav >> role:button[设置]');
      expect(app.shows('id:login-dialog')).toBe(false);
      await app.press('id:main-nav >> role:button[设置]');
      await app.press('role:button[登录]');
      await app.waitFor('id:login-dialog');
      expect(app.value('id:login-mobile')).toBe('13800138000');
      expect(app.value('id:login-code')).toBe('1234');
      await app.press('role:button[关闭]');
      expect(app.shows('role:combobox[文件命名模板]')).toBe(true);
    },
    60_000,
  );
}
