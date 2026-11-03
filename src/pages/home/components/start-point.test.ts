import { describe, expect, it } from 'vitest';

import { decideStartPoint } from './start-point';

const items = (...ids: string[]) => ids.map((seriesId) => ({ seriesId }));

const base = {
  pinHold: true,
  cursorMoved: false,
  playingId: null,
  cursorSeriesId: 'a',
  storeVidIndex: 0,
  index: 0,
  items: items('a', 'b'),
};

describe('decideStartPoint', () => {
  it('外部点名的目标不在流里：换目标但不撤钉', () => {
    const d = decideStartPoint({
      ...base,
      playingId: 'searched',
      cursorSeriesId: 'searched',
      storeVidIndex: 0,
      cursorMoved: true,
      items: items('a', 'b'),
    });
    expect(d).toEqual({
      kind: 'play',
      seriesId: 'searched',
      releasePin: false,
      anchorIndex: null,
    });
  });

  it('目标在流里才撤钉，并把游标重锚到撤钉后的同一步剧', () => {
    const d = decideStartPoint({
      ...base,
      playingId: 'pinned',
      cursorSeriesId: 'c',
      storeVidIndex: 3,
      cursorMoved: true,
      index: 1,
      items: items('c', 'd'),
    });
    expect(d).toEqual({ kind: 'play', seriesId: 'c', releasePin: true, anchorIndex: 0 });
  });

  it('闸已关的常规换片：不谈撤钉、游标与列表同位不重锚', () => {
    const d = decideStartPoint({
      ...base,
      pinHold: false,
      playingId: 'a',
      cursorSeriesId: 'c',
      storeVidIndex: 2,
      cursorMoved: true,
      index: 1,
      items: items('a', 'c', 'd'),
    });
    expect(d).toEqual({ kind: 'play', seriesId: 'c', releasePin: false, anchorIndex: null });
  });

  it('目标一致且集号已定：不决策（在播不被起点决策拽动）', () => {
    expect(
      decideStartPoint({
        ...base,
        playingId: 'a',
        cursorSeriesId: 'a',
        storeVidIndex: 5,
        items: items('a', 'b'),
      }),
    ).toEqual({ kind: 'wait' });
  });

  it('在播保护期游标没动、内容被顶掉：对齐游标回在播剧', () => {
    const d = decideStartPoint({
      ...base,
      playingId: 'p',
      cursorSeriesId: 'x',
      storeVidIndex: 2,
      items: items('x', 'p', 'y'),
      index: 2,
    });
    expect(d).toEqual({ kind: 'protect', anchorIndex: 1 });
  });

  it('保护期在播剧已不在流里：游标夹回钉位 0', () => {
    const d = decideStartPoint({
      ...base,
      playingId: 'p',
      cursorSeriesId: 'x',
      storeVidIndex: 2,
      items: items('x', 'y'),
    });
    expect(d).toEqual({ kind: 'protect', anchorIndex: 0 });
  });

  it('游标动过则保护让位、照旧拽目标换片', () => {
    const d = decideStartPoint({
      ...base,
      playingId: 'p',
      cursorSeriesId: 'x',
      storeVidIndex: 2,
      cursorMoved: true,
      items: items('x', 'p', 'y'),
      index: 1,
    });
    expect(d).toEqual({ kind: 'play', seriesId: 'x', releasePin: true, anchorIndex: 0 });
  });

  it('只点名剧（集号 0）：走起点决策补集号', () => {
    const d = decideStartPoint({
      ...base,
      playingId: 'a',
      cursorSeriesId: 'a',
      storeVidIndex: 0,
      cursorMoved: true,
    });
    expect(d).toEqual({ kind: 'play', seriesId: 'a', releasePin: true, anchorIndex: null });
  });
});
