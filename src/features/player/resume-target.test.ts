import { describe, expect, it } from 'vitest';
import { resolveHistoryTarget, resolveResumeTarget } from './resume-target';

const episodes = [
  { vidIndex: 1, vid: 'first' },
  { vidIndex: 8, vid: 'middle' },
  { vidIndex: 128, vid: 'last' },
];

const series = { seriesId: 'series', episodes };
const history = { seriesId: 'series', vid: 'last', vidIndex: 129, positionMs: 120_000 };

describe('resolveHistoryTarget', () => {
  it('uses the exact video identity when the history index exceeds the episode count', () => {
    expect(resolveHistoryTarget(history, series)).toEqual({ vidIndex: 128, positionMs: 120_000 });
  });

  it('keeps the matching middle episode and its playback position', () => {
    expect(
      resolveHistoryTarget({ ...history, vid: 'middle', vidIndex: 9, positionMs: 35_000 }, series),
    ).toEqual({ vidIndex: 8, positionMs: 35_000 });
  });

  it('does not substitute another episode when a stored video has disappeared', () => {
    expect(resolveHistoryTarget({ ...history, vid: 'removed', vidIndex: 8 }, series)).toBeNull();
  });

  it('accepts an index without a video identity only when that episode exists', () => {
    expect(resolveHistoryTarget({ ...history, vid: '', vidIndex: 8 }, series)).toEqual({
      vidIndex: 8,
      positionMs: 120_000,
    });
    expect(resolveHistoryTarget({ ...history, vid: '' }, series)).toBeNull();
    expect(resolveHistoryTarget({ ...history, vid: '', vidIndex: 0 }, series)).toBeNull();
  });

  it('rejects an empty archive, another series, and an episode without a video', () => {
    expect(resolveHistoryTarget(history, { ...series, episodes: [] })).toBeNull();
    expect(resolveHistoryTarget(history, { ...series, seriesId: 'another' })).toBeNull();
    expect(
      resolveHistoryTarget(
        { ...history, vid: '', vidIndex: 1 },
        { ...series, episodes: [{ vidIndex: 1, vid: '' }] },
      ),
    ).toBeNull();
  });

  it('resolves a one-episode archive without changing its episode number', () => {
    expect(
      resolveHistoryTarget(
        { ...history, vid: 'only', vidIndex: 2 },
        { ...series, episodes: [{ vidIndex: 1, vid: 'only' }] },
      ),
    ).toEqual({ vidIndex: 1, positionMs: 120_000 });
  });
});

describe('resolveResumeTarget', () => {
  const local = { vidIndex: 8, currentTime: 10, updatedAt: 1000 };
  const cloud = { ...history, updatedAtMs: 2000 };

  it('corrects a newer cloud target by video identity before entering home playback', () => {
    expect(resolveResumeTarget(series, local, cloud)).toEqual({
      vidIndex: 128,
      positionMs: 120_000,
    });
  });

  it('keeps a newer valid local position without consuming the older cloud position', () => {
    expect(resolveResumeTarget(series, { ...local, updatedAt: 3000 }, cloud)).toEqual({
      vidIndex: 8,
      positionMs: 0,
    });
  });

  it('does not silently switch episodes when the chosen history video has disappeared', () => {
    expect(resolveResumeTarget(series, local, { ...cloud, vid: 'removed' })).toBeNull();
    expect(
      resolveResumeTarget(series, { ...local, vidIndex: 129, updatedAt: 3000 }, cloud),
    ).toBeNull();
  });

  it('starts a new series from its first actual episode and rejects an empty archive', () => {
    expect(resolveResumeTarget({ ...series, episodes: [episodes[1]!] }, null, null)).toEqual({
      vidIndex: 8,
      positionMs: 0,
    });
    expect(resolveResumeTarget({ ...series, episodes: [] }, null, null)).toBeNull();
  });
});
