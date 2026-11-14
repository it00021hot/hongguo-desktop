import { expect, open, test as registerTest } from 'nsh/test';
const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

test('native visibility pauses under another process opaque window and preserves user pause', async () => {
  const app = await open('..', {
    width: 640,
    height: 480,
    platform: 'macos',
    locale: 'en-US',
    colorScheme: 'dark',
  });
  await app.waitFor('id:player-stage');
  await app.keyboard.press('mod+shift+v');
  await app.waitFor('text:playback:playing');
  await app.press('role:button[Minimize briefly]');
  await app.waitFor('text:playback:paused:window');
  await app.waitFor('text:playback:playing');
  await app.press('role:button[Cover partly]');
  await app.wait(3500);
  expect(app.shows('text:playback:playing')).toBe(true);
  await app.press('role:button[Cover fully]');
  await app.waitFor('text:exposed:false');
  await app.waitFor('text:playback:paused:window');
  await app.waitFor('text:playback:playing');
  await app.press('role:button[Toggle user pause]');
  await app.press('role:button[Cover fully]');
  await app.waitFor('text:exposed:false');
  await app.waitFor('text:exposed:true');
  await app.waitFor('text:playback:paused:user');
  await app.press('role:button[Toggle user pause]');
  await app.press('role:button[Hide briefly]');
  await app.waitFor('text:playback:paused:window');
  await app.waitFor('text:playback:playing');
  await app.press('role:button[Toggle auto pause]');
  await app.press('role:button[Cover fully]');
  await app.wait(3500);
  expect(app.shows('text:playback:playing')).toBe(true);
}, 60_000);
