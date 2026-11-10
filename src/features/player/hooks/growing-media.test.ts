import { describe, expect, it } from 'vitest';
import { offlineQa } from '@/utils/offline-qa';
import { classifyGrow, decidePullAction, shouldNotifyUnavailable } from './growing-media';

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

  it('treats legitimate 416 Range Not Satisfiable as finished, not failed', () => {
    // Done 后 bytes=past- 与空成片 suffix 均由后端回 416；不能误触发失败回退
    expect(classifyGrow(416, null, 0, 17)).toEqual({ kind: 'finished' });
    expect(classifyGrow(416, null, 0, 0)).toEqual({ kind: 'finished' });
    expect(decidePullAction(classifyGrow(416, null, 0, 10))).toEqual({
      action: 'finish',
    });
  });

  it('still fails unknown non-206 statuses that are not 416/204/409/500', () => {
    expect(classifyGrow(404, null, 0, 0)).toEqual({ kind: 'failed' });
    expect(classifyGrow(200, null, 8, 0)).toEqual({ kind: 'failed' });
  });
});

describe('pull grow terminal → onUnavailable', () => {
  it('requires onUnavailable for cancelled and failed grow steps', () => {
    expect(shouldNotifyUnavailable({ kind: 'cancelled' })).toBe(true);
    expect(shouldNotifyUnavailable({ kind: 'failed' })).toBe(true);
    expect(shouldNotifyUnavailable({ kind: 'finished' })).toBe(false);
    expect(shouldNotifyUnavailable({ kind: 'data', nextOffset: 1, finished: true })).toBe(false);
  });

  it('maps cancelled/failed pull decisions to unavailable, not silent return', () => {
    expect(decidePullAction(classifyGrow(409, 'cancelled', 0, 4))).toEqual({
      action: 'unavailable',
    });
    expect(decidePullAction(classifyGrow(500, 'failed', 0, 4))).toEqual({
      action: 'unavailable',
    });
    expect(decidePullAction(classifyGrow(204, 'done', 0, 4))).toEqual({
      action: 'finish',
    });
    expect(decidePullAction(classifyGrow(206, 'open', 8, 0))).toEqual({
      action: 'append',
      nextOffset: 8,
      thenFinish: false,
    });
    expect(decidePullAction(classifyGrow(206, 'done', 3, 8))).toEqual({
      action: 'append',
      nextOffset: 11,
      thenFinish: true,
    });
  });
});

describe('offlineQa', () => {
  it('stays off unless the dev command sets the switch', () => {
    expect(offlineQa()).toBe(false);
  });
});
