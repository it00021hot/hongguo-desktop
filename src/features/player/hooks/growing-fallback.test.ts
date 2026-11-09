import { describe, expect, it } from 'vitest';
import {
  cancelProducerOn,
  cancelProducerOnMseCleanup,
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
