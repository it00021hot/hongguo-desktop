import { describe, expect, it } from 'vitest';
import { offlineQa } from '@/utils/offline-qa';
import { classifyGrow } from './growing-media';

describe('classifyGrow', () => {
  it('treats one 206 as a chunk and keeps reading', () => {
    expect(classifyGrow(206, 'open', 12, 0)).toEqual({
      kind: 'data',
      nextOffset: 12,
      finished: false,
    });
  });

  it('stops after the chunk that reaches the finished file', () => {
    expect(classifyGrow(206, 'done', 5, 12)).toEqual({
      kind: 'data',
      nextOffset: 17,
      finished: true,
    });
    expect(classifyGrow(204, 'done', 0, 17)).toEqual({ kind: 'finished' });
  });

  it('does not treat cancel or failure as a short successful movie', () => {
    expect(classifyGrow(409, 'cancelled', 0, 12)).toEqual({ kind: 'cancelled' });
    expect(classifyGrow(500, 'failed', 0, 12)).toEqual({ kind: 'failed' });
  });
});

describe('offlineQa', () => {
  it('stays off unless the dev command sets the switch', () => {
    expect(offlineQa()).toBe(false);
  });
});
