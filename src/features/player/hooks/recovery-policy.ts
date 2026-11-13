/** 播放失败恢复策略：错误分类 + 失败账本 → 恢复动作（纯函数）。 */

import type { MediaErrorKind } from './media-error';

/** 恢复动作。播放不转码，没有「换路兜底」这一档：重试不行就如实报错。 */
export type RecoveryAction = 'retry' | 'errorPage';

/** 当前集上各类错误的已发生次数。 */
export type FailureCounts = Record<MediaErrorKind, number>;

/** 每类错误的重试预算：第二次同类失败才升级动作。 */
const RETRY_BUDGET_PER_KIND = 1;

/** 空账本（换集后的起点）。 */
export function emptyFailureCounts(): FailureCounts {
  return { decode: 0, network: 0, other: 0 };
}

/**
 * 裁决一次在线播放失败后的恢复动作。decode（3/4）不是单一语义：确定性失败与瞬时管线故障都落这，
 * 重取一条防缓存钉的新流能否存活是判别器——每类错误先重试一次，复发就进错误页。
 */
export function recoveryAction(kind: MediaErrorKind, counts: FailureCounts): RecoveryAction {
  return counts[kind] < RETRY_BUDGET_PER_KIND ? 'retry' : 'errorPage';
}
