import { expect, open, test as registerTest } from 'nsh/test';
import {
  episode,
  sameEpisode,
  startPreviouslyFailingSource,
  timeline,
  waitForPlaying,
} from './helpers';

const test: (name: string, body: () => Promise<void>, timeout?: number) => void = registerTest;

test('production progressive playback rechecks the previously failing real source', async () => {
  const app = await open('.', {
    width: 1280,
    height: 840,
    colorScheme: 'dark',
    locale: 'zh-CN',
    network: 'live',
  });
  try {
    await app.waitFor('id:main-nav', { timeout: 30_000 });
    await app.press('id:main-nav >> role:button[设置]');
    await app.waitFor('role:combobox[文件命名模板]');
    await app.scroll('id:preferences-form >> id:form-scroll', 757);
    await app.press('role:switch[最小化时自动暂停]');
    await app.scroll('id:preferences-form >> id:form-scroll', 5000);
    await app.press('role:button[保存]');
    await app.waitFor('text:设置已保存');
    const started = await startPreviouslyFailingSource(app);
    await waitForPlaying(app);
    expect(app.texts().some((text) => text.startsWith('@持械入宋'))).toBe(true);
    const original = episode(app);
    const first = timeline(app);
    expect(first !== null && first.current >= 2 && first.current <= 4).toBe(true);
    console.log(
      'PROGRESSIVE_STARTED_JSON ' +
        JSON.stringify({
          elapsedMs: Date.now() - started,
          observedAtMs: Date.now(),
          timeline: first,
          episode: original,
        }),
    );
    await app.wait(12_000);
    sameEpisode(app, original);
    expect(timeline(app)!.current - first!.current >= 8).toBe(true);
    console.log(
      'PROGRESSIVE_VERIFICATION_JSON ' +
        JSON.stringify({ initial: first, actual: timeline(app), episode: original }),
    );
    await app.screenshot('target/nsh-playback/progressive-playing.png');
  } catch (error) {
    console.log(
      'PROGRESSIVE_FAILURE_JSON ' + JSON.stringify({ message: String(error), texts: app.texts() }),
    );
    await app.screenshot('target/nsh-playback/progressive-failed.png');
    throw error;
  }
}, 180_000);
