import { expect, open, test as registerTest } from 'nsh/test';

// NSH 0.4 awaits test promises, although its generated callback type returns void.
const test: (name: string, body: () => Promise<void>) => void = registerTest;

test('light controls respond to presses', async () => {
  const app = await open('.', { colorScheme: 'light', locale: 'zh-CN' });
  await app.press('role:button[播放]');
  expect(app.shows('id:preview-status >> text:播放')).toBe(true);
  await app.press('id:main-nav >> role:button[历史]');
  await app.screenshot('target/nsh-preview/light.png');
  await app.press('role:button[筛选]');
  await app.waitFor('role:button[关闭]');
  await app.screenshot('target/nsh-preview/sheet-light.png');
  await app.keyboard.press('Escape');
  expect(app.shows('role:button[关闭]')).toBe(false);
});

test('dark controls stay readable', async () => {
  const app = await open('.', { colorScheme: 'dark', locale: 'zh-CN' });
  expect(app.texts()).toContain('红果播放器');
  await app.type('id:preview-query', '暗色文字');
  expect(app.value('id:preview-query')).toBe('暗色文字');
  await app.screenshot('target/nsh-preview/dark.png');
  await app.press('role:switch[播完自动下一集]');
  await app.press('role:button[筛选]');
  await app.waitFor('role:button[关闭]');
  await app.screenshot('target/nsh-preview/sheet-dark.png');
  await app.press('role:button[关闭]');
});

test('desktop light controls support keyboard and navigation', async () => {
  const app = await open('.', { width: 1100, height: 760, colorScheme: 'light', locale: 'en-US' });
  await app.type('id:preview-query', 'drama');
  await app.keyboard.press('tab');
  await app.keyboard.press('enter');
  expect(app.shows('id:preview-status >> text:Play')).toBe(true);
  await app.hover('id:main-nav >> role:button[History]');
  await app.press('id:main-nav >> role:button[History]');
  await app.screenshot('target/nsh-preview/desktop-light.png');
});

test('desktop dark controls stay usable with reduced motion', async () => {
  const app = await open('.', {
    width: 1100,
    height: 760,
    colorScheme: 'dark',
    locale: 'en-US',
    reducedMotion: true,
  });
  await app.type('id:preview-query', 'Readable dark input');
  expect(app.value('id:preview-query')).toBe('Readable dark input');
  await app.press('role:button[Play]');
  expect(app.shows('id:preview-status >> text:Play')).toBe(true);
  await app.screenshot('target/nsh-preview/desktop-dark.png');
});
