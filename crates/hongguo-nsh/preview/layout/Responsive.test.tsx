import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

test('narrow desktop uses the five production navigation destinations', async () => {
  const app = await open('.', {
    width: 480,
    height: 840,
    colorScheme: 'dark',
    locale: 'zh-CN',
  });
  await app.waitFor('text:00:42');
  await app.waitFor('text:02:49');
  const footer = 'id:bottom-nav';
  expect(app.count(footer + ' >> role:button')).toBe(5);
  for (const label of ['推荐', '找剧', '排行榜', '历史', '登录']) {
    expect(app.shows(footer + ' >> role:button[' + label + ']')).toBe(true);
  }
  expect(app.shows('id:main-nav >> role:button[下载管理]')).toBe(false);
  await app.hover('id:app-toolbar-rest');
  await app.wait(250);
  await app.screenshot('target/nsh-preview/home-dark-expanded-480x840.png');

  await app.press(footer + ' >> role:button[历史]');
  await app.waitFor('text:看到第1集/共80集 00:42');
  expect(app.shows('role:button[管理]')).toBe(true);
  expect(app.count(footer + ' >> role:button')).toBe(5);
  await app.hover('id:app-toolbar-rest');
  await app.screenshot('target/nsh-preview/history-dark-expanded-480x840.png');

  await app.press(footer + ' >> role:button[登录]');
  await app.waitFor('id:preferences-form');
  expect(app.shows('id:login-dialog')).toBe(false);
  await app.press('id:preferences-form >> role:button[登录]');
  await app.waitFor('id:login-dialog');
  expect(app.enabled('id:login-dialog >> role:button[登录]')).toBe(false);
  await app.hover('id:login-dialog');
  await app.screenshot('target/nsh-preview/account-login-dark-480x840.png');
  await app.keyboard.press('escape');
  await app.waitFor('id:login-dialog', { state: 'gone' });
  expect(app.count(footer + ' >> role:button')).toBe(5);
});

test('compact desktop home preserves the production player layout', async () => {
  const app = await open('.', {
    width: 1100,
    height: 760,
    colorScheme: 'dark',
    locale: 'zh-CN',
  });
  await app.waitFor('text:00:42 / 02:49');
  expect(app.shows('id:player-stage')).toBe(true);
  expect(app.shows('id:main-nav >> role:button[历史]')).toBe(true);
  expect(app.shows('id:bottom-nav >> role:button[历史]')).toBe(false);
  await app.hover('id:app-toolbar-rest');
  await app.wait(250);
  await app.screenshot('target/nsh-preview/home-dark-expanded.png');
});
