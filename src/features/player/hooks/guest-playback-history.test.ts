import { describe, expect, it } from 'vitest';
import { GuestPlaybackHistory } from './guest-playback-history';

const episode = (vidIndex: number, positionMs = 0) => ({
  seriesId: '100',
  vidIndex,
  vid: String(100 + vidIndex),
  positionMs,
  durationMs: 90_000,
  meta: { title: 'series', cover: '', episodeCnt: 14 },
});

describe('anonymous playback history', () => {
  it('reports zero on entry, keeps cumulative playback on switch and batches on exit', () => {
    const history = new GuestPlaybackHistory(new Map());
    const [first] = history.begin(episode(1), 1_000);
    expect(first?.meta.guest?.progress).toMatchObject([{ positionMs: 0, cumulativeMs: 0 }]);
    history.observe(15_559, 90_000, 17_000);

    const [next] = history.begin(episode(13), 18_000);
    expect(next).toMatchObject({
      positionMs: 0,
      meta: { guest: { cumulativeMs: 15_559, progress: [] } },
    });
    history.observe(7_000, 90_000, 25_000);
    history.seek();
    history.observe(40_000, 90_000, 30_000);
    history.observe(41_000, 90_000, 31_000);

    expect(history.finish()).toMatchObject({
      positionMs: 41_000,
      meta: {
        guest: {
          cumulativeMs: 23_559,
          progress: [
            { vid: '113', positionMs: 41_000, cumulativeMs: 23_559, observedAtMs: 31_000 },
            { vid: '101', positionMs: 15_559, cumulativeMs: 15_559, observedAtMs: 17_000 },
          ],
        },
      },
    });
  });

  it('keeps the position and cumulative time when reopening, with no short exit upload', () => {
    const totals = new Map<string, number>();
    const first = new GuestPlaybackHistory(totals);
    first.begin(episode(13), 1_000);
    first.observe(20_000, 90_000, 21_000);
    expect(first.finish()?.positionMs).toBe(20_000);

    const next = new GuestPlaybackHistory(totals);
    expect(next.begin(episode(13, 20_000), 22_000)[0]).toMatchObject({
      positionMs: 20_000,
      meta: { guest: { cumulativeMs: 20_000, progress: [] } },
    });
    next.observe(23_000, 90_000, 25_000);
    expect(next.finish()).toBeNull();
  });

  it('closes the old series and includes both series in the progress batch', () => {
    const history = new GuestPlaybackHistory(new Map());
    history.begin(episode(13), 1_000);
    history.observe(10_000, 90_000, 11_000);

    const seasonTwo = { ...episode(1), seriesId: '200', vid: '201' };
    expect(history.begin(seasonTwo, 12_000)).toMatchObject([
      { seriesId: '100', positionMs: 10_000, meta: { guest: { progress: [] } } },
      {
        seriesId: '200',
        positionMs: 0,
        meta: {
          guest: {
            cumulativeMs: 0,
            progress: [
              { seriesId: '200', vid: '201', positionMs: 0 },
              { seriesId: '100', vid: '113', positionMs: 10_000 },
            ],
          },
        },
      },
    ]);

    history.observe(7_000, 90_000, 19_000);
    expect(history.begin(episode(13, 10_000), 20_000)).toMatchObject([
      { seriesId: '200', positionMs: 7_000, meta: { guest: { progress: [] } } },
      {
        seriesId: '100',
        positionMs: 10_000,
        meta: {
          guest: {
            cumulativeMs: 10_000,
            progress: [
              { seriesId: '100', vid: '113', positionMs: 10_000 },
              { seriesId: '200', vid: '201', positionMs: 7_000 },
            ],
          },
        },
      },
    ]);
  });
});
