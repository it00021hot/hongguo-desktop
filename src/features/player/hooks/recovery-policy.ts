/** 播放失败恢复策略：错误分类 + 失败账本 → 恢复动作（纯函数）。 */

import type { MediaErrorKind } from './media-error';

/** 恢复动作。 */
export type RecoveryAction = 'retry' | 'fallback' | 'errorPage';

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
 * 重取一条防缓存钉的新流能否存活是判别器——decode 首次与其他错误一样先重试，复发才升级整集转码兜底。
 *
 * @param knownUnplayable 能力探测已判定本机 WebView 解不了（Android 软解 / 已授权兼容会话）：
 *   decode 不再烧一次重试预算，径直兜底——重取同一条 HEVC 流只会再黑屏一次。
 */
export function recoveryAction(
  kind: MediaErrorKind,
  counts: FailureCounts,
  knownUnplayable = false,
): RecoveryAction {
  if (knownUnplayable && kind === 'decode') return 'fallback';
  if (counts[kind] < RETRY_BUDGET_PER_KIND) return 'retry';
  return kind === 'decode' ? 'fallback' : 'errorPage';
}
