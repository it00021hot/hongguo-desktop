import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

test('production seek rail receives native pointer drags at its visible center', async () => {
  const app = await open('.', { width: 1280, height: 840, colorScheme: 'dark', locale: 'zh-CN' });
  await app.waitFor('text:00:42 / 02:49');
  await app.hover('role:adjustable[Playback position]');
  await app.drag('role:adjustable[Playback position]', { x: 64, y: 0 });
  console.log('SEEK_THUMB_POINTER_JSON ' + JSON.stringify({ texts: app.texts() }));
  await app.drag('id:player-seek-rail', { x: 64, y: 0 });
  await app.waitFor('text:01:35 / 02:49');
  await app.drag('id:player-seek-rail', { x: -64, y: 0 });
  await app.waitFor('text:01:14 / 02:49');
  expect(app.shows('role:button[播放]')).toBe(true);
  await app.hover('id:app-toolbar-rest');
  await app.screenshot('target/nsh-preview/seek-rail-native.png');
});
