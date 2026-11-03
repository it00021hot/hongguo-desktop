/**
 * 首页起点决策的纯决策核（home-page 起点 effect 的判定部分）。输入是某一拍的快照，
 * 输出要不要动作、做什么，不含 setState。三个关键语义（改动前先读同名 test）：
 * 目标一致且集号已定不决策；保护期游标没动且内容被顶掉则对齐游标回在播剧；
 * 撤钉前提是目标剧在未钉住的列表里（否则它从列表消失、下一拍播放目标被抢走）。
 */

export interface StartPointSnapshot {
  pinHold: boolean;
  /** 游标动过且信号未被消费（数据未就绪的早退不许吃掉它，见 effect 侧注释） */
  cursorMoved: boolean;
  /** 在播剧（store 目标）；null = 无目标 */
  playingId: string | null;
  /** 游标处的剧（其分集档案已就绪才有决策） */
  cursorSeriesId: string;
  /** store 目标集号（0 = 只点名了剧，集号待起点决策补上） */
  storeVidIndex: number;
  /** 当前游标下标（displayItems 口径） */
  index: number;
  /** 未钉住的列表（撤钉后的原位语义；关闸判据与重锚都看它） */
  items: readonly { seriesId: string }[];
}

export type StartPointDecision =
  /** 这一拍不动作（起点已定等；effect 直接返回，信号不消费） */
  | { kind: 'wait' }
  /** 对齐游标回在播剧（纯 UI 写，一轮收敛） */
  | { kind: 'protect'; anchorIndex: number }
  /**
   * 播放目标对到游标处的剧。releasePin = 撤掉在播钉（目标在流里原位存在时才 true）；
   * anchorIndex = 撤钉后游标需重锚到的下标（无移位为 null）。
   */
  | { kind: 'play'; seriesId: string; releasePin: boolean; anchorIndex: number | null };

export function decideStartPoint(s: StartPointSnapshot): StartPointDecision {
  // 目标剧一致且集号已指定：起点已定，历史/进度刷新不得拽走目标
  if (s.playingId === s.cursorSeriesId && s.storeVidIndex > 0) {
    return { kind: 'wait' };
  }
  // 在播保护期且游标没动、游标处的剧被顶掉：对齐游标回在播剧、绝不拽播放目标
  // （这里若换片，钉住把它挪出游标位、游标处又填入别的剧，effect 重跑再拽，永不收敛）
  if (s.pinHold && !s.cursorMoved && s.playingId && s.playingId !== s.cursorSeriesId) {
    const anchor = s.items.findIndex((i) => i.seriesId === s.playingId);
    return { kind: 'protect', anchorIndex: Math.max(anchor, 0) };
  }
  // 用户切剧 / 外部点名 / 起点待定（集号 0）：播放目标对到游标处的剧。
  const anchor = s.items.findIndex((i) => i.seriesId === s.cursorSeriesId);
  return {
    kind: 'play',
    seriesId: s.cursorSeriesId,
    releasePin: s.pinHold && anchor >= 0,
    anchorIndex: anchor >= 0 && anchor !== s.index ? anchor : null,
  };
}
