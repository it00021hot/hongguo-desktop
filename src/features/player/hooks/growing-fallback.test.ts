import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  cancelProducerOn,
  cancelProducerOnMseCleanup,
  resetPendingProducerCancelsForTests,
  retainProducerOnRemount,
  scheduleProducerCancel,
  wholeFileSrc,
} from './growing-fallback';

const payload = 'hongguo-local://localhost/f/complete';

describe('whole file fallback', () => {
  it('waits for a completed file and ignores a growing or cancelled path', () => {
    expect(
      wholeFileSrc({
        episodeKey: 's:1',
        eventKey: 's:1',
        producer: 'running',
        completeUrl: payload,
        waiting: true,
      }),
    ).toBeNull();
    expect(
      wholeFileSrc({
        episodeKey: 's:1',
        eventKey: 's:1',
        producer: 'cancelled',
        completeUrl: payload,
        waiting: true,
      }),
    ).toBeNull();
    expect(
      wholeFileSrc({
        episodeKey: 's:1',
        eventKey: 's:1',
        producer: 'failed',
        completeUrl: payload,
        waiting: true,
      }),
    ).toBeNull();
    expect(
      wholeFileSrc({
        episodeKey: 's:2',
        eventKey: 's:1',
        producer: 'completed',
        completeUrl: payload,
        waiting: true,
      }),
    ).toBeNull();
    expect(
      wholeFileSrc({
        episodeKey: 's:1',
        eventKey: 's:1',
        producer: 'completed',
        completeUrl: payload,
        waiting: true,
      }),
    ).toBe(payload);
  });

  it('does not cancel the producer when MSE fails or the MSE effect cleans up', () => {
    expect(cancelProducerOn('mse-failed')).toBe(false);
    expect(cancelProducerOn('leave')).toBe(true);
    expect(cancelProducerOnMseCleanup('mse-effect')).toBe(false);
    expect(cancelProducerOnMseCleanup('episode-leave')).toBe(true);
  });
});

describe('remount-safe producer cancel', () => {
  afterEach(() => {
    resetPendingProducerCancelsForTests();
  });

  it('cancels the old definition when another definition of the same episode opens', async () => {
    const cancel = vi.fn();
    scheduleProducerCancel('s', 1, cancel, 720);
    retainProducerOnRemount('s', 1, 1080);
    await new Promise((resolve) => setTimeout(resolve, 220));
    expect(cancel).toHaveBeenCalledWith({ seriesId: 's', vidIndex: 1, definition: 720 });
  });

  it('does not fire cancel when the same episode remounts within the grace window', async () => {
    const calls: string[] = [];
    scheduleProducerCancel('s', 1, ({ seriesId, vidIndex }) => {
      calls.push(`${seriesId}:${vidIndex}`);
    });
    retainProducerOnRemount('s', 1);
    await new Promise((r) => setTimeout(r, 220));
    expect(calls).toEqual([]);
  });

  it('fires cancel after grace when nothing remounts', async () => {
    const calls: string[] = [];
    scheduleProducerCancel('s', 2, ({ seriesId, vidIndex }) => {
      calls.push(`${seriesId}:${vidIndex}`);
    });
    await new Promise((r) => setTimeout(r, 220));
    expect(calls).toEqual(['s:2']);
  });
});
