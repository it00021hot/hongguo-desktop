import { expect, open, test as registerTest } from 'nsh/test';
import { seekAndConfirm, waitForPlaying } from './helpers';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

test('production seek confirms native progress within the same episode', async () => {
  const app = await open('.', {
    width: 1280,
    height: 840,
    colorScheme: 'dark',
    locale: 'zh-CN',
    network: 'live',
  });
  try {
    await waitForPlaying(app);
    const result = await seekAndConfirm(app);
    expect(Math.abs(result.forward.current - result.expected) <= 2).toBe(true);
    console.log('SEEK_VERIFICATION_JSON ' + JSON.stringify(result));
    await app.screenshot('target/nsh-playback/seek-confirmed.png');
  } catch (error) {
    console.log(
      'SEEK_FAILURE_JSON ' + JSON.stringify({ message: String(error), texts: app.texts() }),
    );
    await app.screenshot('target/nsh-playback/seek-failed.png');
    throw error;
  }
}, 180_000);
