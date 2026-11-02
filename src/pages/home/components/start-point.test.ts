import { describe, expect, it } from 'vitest';

import { decideStartPoint } from './start-point';

/** 未钉住的列表工厂（items 口径，不含钉住条目）。 */
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
  // 主 bug 复现：搜索/榜单等外部点名进首页，目标剧不在推荐流里（只以钉住
  // 占位存在）。旧逻辑无条件关闸，目标从列表消失、游标 0 落到别的剧，
  // 下一拍起点决策把播放目标抢走——「点开搜索结果播的是别的剧」。
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
    // 钉住条目在游标之前：撤钉后内容整体左移一格，游标必须按内容跟移
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

  // 保护分支：换一批/钉住移位把游标处的剧顶掉而游标没动——对齐游标回
  // 在播剧，绝不拽播放目标（「拽 → 移位 → 再拽」轮回的闸门）。
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

  // 游标动过 = 用户切剧，保护让位。信号会被「档案/进度未就绪」的早退
  // 晚一拍才送达决策，此时必须仍然按用户切剧处理（旧 idxChanged 逐拍
  // 重算，早退把它吃掉后保护误伤，滚轮被弹回在播剧）。
  // 坐标口径：displayItems = [钉住的 p] + items，滚到 x 时游标在 1。
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
