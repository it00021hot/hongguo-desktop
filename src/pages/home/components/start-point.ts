/**
 * 首页起点决策的纯决策核（`home-page` 起点 effect 的判定部分）。
 *
 * 抽成纯函数的原因：这条判定链是「搜索点开播错剧 / 刷剧被自动换片 /
 * 保护分支轮回」三类事故的共同现场，effect 内联时只能靠整页手测回归，
 * 抽出来后每次事故都能落一条复现测试钉死（本目录同名 test）。
 *
 * 输入是 effect 某一拍读到的快照；输出是**要不要动作、做什么动作**，
 * 不含任何 setState——装配（何时调、命中后写哪些状态）留在 effect 里。
 *
 * 三个关键语义（改动前先读对应复现测试）：
 * - 目标剧一致且集号已定 → 不决策：在播不被任何背景刷新拽动；
 * - 钉住保护期游标没动、游标处内容被顶掉 → 对齐游标回在播剧，绝不换片；
 * - 撤钉的**前提**是目标剧在未钉住的列表里——外部点名（搜索/榜单）的
 *   目标只以钉住占位存在，撤了它就从列表消失，游标 0 落到别的剧，
 *   下一拍就把播放目标抢走；撤钉同时按内容把游标重锚（钉位在游标
 *   之前时，撤钉会让内容左移一格）。
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
   * 播放目标对到游标处的剧。`releasePin` = 撤掉在播钉（目标在流里原位
   * 存在时才 true）；`anchorIndex` = 撤钉后游标需要重锚到的下标（无移位
   * 为 null，不写游标）。
   */
  | { kind: 'play'; seriesId: string; releasePin: boolean; anchorIndex: number | null };

export function decideStartPoint(s: StartPointSnapshot): StartPointDecision {
  // 目标剧一致且集号已指定：起点已定，历史/进度刷新不得拽走目标
  if (s.playingId === s.cursorSeriesId && s.storeVidIndex > 0) {
    return { kind: 'wait' };
  }
  // 在播保护期（pinHold）且游标没动、游标处的剧却被顶掉（钉住条目随
  // playingId 移位/换批重排）：对齐游标回在播剧、绝不拽播放目标——
  // 这里若换片，钉住会把它挪出游标位，游标处又填入别的剧，effect 重跑
  // 再拽，永不收敛（切剧 Maximum update depth 崩溃的根源）。
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
