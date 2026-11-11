import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CompatProgress } from '@/service/schema';
import {
  cancelProducerOn,
  cancelProducerOnMseCleanup,
  compatMediaMode,
  mergeCompatProgress,
  resetPendingProducerCancelsForTests,
  retainProducerOnRemount,
  scheduleProducerCancel,
  visibleCompatProgress,
  wholeFileSrc,
} from './growing-fallback';

const key = 's:1:720';

describe('compat playback', () => {
  it('keeps consuming the growing stream when the producer finishes', () => {
    expect(compatMediaMode(true, false, 'transcoding')).toBe('growing');
    expect(compatMediaMode(true, false, 'completed')).toBe('growing');
  });

  it('opens the whole file after the growing consumer fails and the producer finishes', () => {
    expect(compatMediaMode(true, true, 'transcoding')).toBe('waiting');
    expect(compatMediaMode(true, true, 'failed')).toBe('waiting');
    expect(compatMediaMode(true, true, 'completed')).toBe('file');
    expect(compatMediaMode(false, false, undefined)).toBe('file');
  });

  it('keeps startup progress monotonic through ready and out-of-order events', () => {
    const transcoding: CompatProgress = { key, percent: 77, phase: 'transcoding' };
    const ready: CompatProgress = { key, percent: 99, phase: 'ready' };
    const completed: CompatProgress = { key, percent: 100, phase: 'completed' };
    expect(mergeCompatProgress(transcoding, ready)).toBe(transcoding);
    expect(mergeCompatProgress(transcoding, { ...transcoding, percent: 65 })).toBe(transcoding);
    expect(mergeCompatProgress(transcoding, { ...transcoding, percent: 80 })).toEqual({
      ...transcoding,
      percent: 80,
    });
    expect(mergeCompatProgress(transcoding, completed)).toBe(completed);
    expect(mergeCompatProgress(completed, transcoding)).toBe(completed);
  });

  it('shows the overlay only while playback is waiting for an unfinished producer', () => {
    const progress: CompatProgress = { key, percent: 77, phase: 'transcoding' };
    expect(visibleCompatProgress(progress, false)).toBe(progress);
    expect(visibleCompatProgress(progress, true)).toBeNull();
    expect(visibleCompatProgress({ key, percent: 100, phase: 'completed' }, false)).toBeNull();
  });
});

describe('whole file fallback', () => {
  it('only opens a completed file from the matching offline QA job', () => {
    const completeUrl = 'hongguo-local://localhost/f/complete';
    const completed = { episodeKey: key, eventKey: key, completeUrl, waiting: true } as const;
    for (const producer of ['running', 'cancelled', 'failed'] as const) {
      expect(wholeFileSrc({ ...completed, producer })).toBeNull();
    }
    expect(wholeFileSrc({ ...completed, eventKey: 's:2:720', producer: 'completed' })).toBeNull();
    expect(wholeFileSrc({ ...completed, producer: 'completed' })).toBe(completeUrl);
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
