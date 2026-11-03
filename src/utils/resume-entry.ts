/**
 * 续播起点决策（单一实现）：本地断点（5 秒一写但只覆盖本应用）与云端历史（约 1 分钟一报但跨端）
 * 谁更新用谁；都没有（>3 秒才算看过）回落第 1 集。与后端 `play_service::resume::resolve_entry`
 * 同口径（技术栈不同不共享实现），阈值与取新规则改动两边同步。
 */

export const RESUME_THRESHOLD_SECS = 3;
export const RESUME_THRESHOLD_MS = RESUME_THRESHOLD_SECS * 1000;

interface ResumeLocalProgress {
  vidIndex: number;
  currentTime: number;
  updatedAt: number;
}

interface ResumeHistoryItem {
  vidIndex: number;
  positionMs: number;
  updatedAtMs: number;
}

/**
 * 可续播起点；`null` = 没有可续播记录（调用方回落第 1 集）。云端命中时 positionMs 带集内位置
 * （起播时本地无该集 resumeAt 才消费）；本地命中时 positionMs 恒 0——本地续播位置由后端 resumeAt 直接带回。
 */
export function resolveResumeEntry(
  localProgress: ResumeLocalProgress | null | undefined,
  historyItem: ResumeHistoryItem | null | undefined,
): { index: number; positionMs: number } | null {
  const local =
    localProgress && localProgress.currentTime > RESUME_THRESHOLD_SECS ? localProgress : null;
  const cloud = historyItem && historyItem.positionMs > RESUME_THRESHOLD_MS ? historyItem : null;
  if (cloud && (!local || cloud.updatedAtMs > local.updatedAt)) {
    return { index: Math.max(1, cloud.vidIndex), positionMs: cloud.positionMs };
  }
  if (local) {
    return { index: local.vidIndex, positionMs: 0 };
  }
  return null;
}
