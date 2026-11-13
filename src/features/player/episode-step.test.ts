import { describe, expect, it } from 'vitest';
import { firstEpisodeIndex, resolveEpisodeStep } from './episode-step';

const episodes = [
  { vidIndex: 100, vid: 'last' },
  { vidIndex: 1, vid: 'first' },
  { vidIndex: 7, vid: 'middle' },
];

describe('resolveEpisodeStep', () => {
  it('waits for the episode archive instead of creating episode 101', () => {
    expect(resolveEpisodeStep(undefined, 100, 1)).toEqual({ kind: 'unavailable' });
    expect(resolveEpisodeStep([], 100, 1)).toEqual({ kind: 'unavailable' });
  });

  it('reports the end of the actual archive without inventing another episode', () => {
    expect(resolveEpisodeStep(episodes, 100, 1)).toEqual({ kind: 'last' });
  });

  it('moves across non-contiguous episode numbers in either direction', () => {
    expect(resolveEpisodeStep(episodes, 1, 1)).toEqual({ kind: 'episode', vidIndex: 7 });
    expect(resolveEpisodeStep(episodes, 100, -1)).toEqual({ kind: 'episode', vidIndex: 7 });
    expect(resolveEpisodeStep(episodes, 1, -1)).toEqual({ kind: 'first' });
  });

  it('rejects a stale current target and invalid movement', () => {
    expect(resolveEpisodeStep(episodes, 101, 1)).toEqual({ kind: 'missing' });
    expect(resolveEpisodeStep(episodes, 1, 0)).toEqual({ kind: 'missing' });
  });

  it('handles a single episode and ignores entries without a video', () => {
    const single = [{ vidIndex: 7, vid: 'only' }];
    expect(resolveEpisodeStep(single, 7, 1)).toEqual({ kind: 'last' });
    expect(resolveEpisodeStep(single, 7, -1)).toEqual({ kind: 'first' });
    expect(resolveEpisodeStep([...single, { vidIndex: 8, vid: '' }], 7, 1)).toEqual({
      kind: 'last',
    });
  });
});

describe('firstEpisodeIndex', () => {
  it('takes the first actual episode instead of assuming number one', () => {
    expect(
      firstEpisodeIndex([
        { vidIndex: 13, vid: 'b' },
        { vidIndex: 7, vid: 'a' },
      ]),
    ).toBe(7);
    expect(firstEpisodeIndex([{ vidIndex: 7, vid: '' }])).toBeNull();
    expect(firstEpisodeIndex([])).toBeNull();
  });
});
