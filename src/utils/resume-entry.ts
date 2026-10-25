/**
 * 续播起点决策（单一实现）：本地断点与云端观看历史**谁更新用谁**——
 * 本地档案 5 秒一写但只覆盖本应用（在官方 App 看过的进度它
 * 滞后甚至缺失），云端约 1 分钟一报但跨端；比较双方的更新时间戳取新者，
 * 追更页「识别到 80 集、点播放却回第 1 集」这类跨端进度问题在这一处根治。
 * 都没有（>3 秒才算看过）回落第 1 集。与后端
 * `play_service::resume::resolve_entry` 同口径（技术栈不同不共享实现，
 * 阈值与取新规则改动两边同步）；2026-10-09 用户口径：不看片尾。
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
 * 可续播的起点；`null` = 没有可续播记录（调用方回落第 1 集）。
 *
 * 云端命中时 `positionMs` 带集内位置（起播时本地无该集 resumeAt 才消费）；
 * 本地命中时 `positionMs` 恒为 0：本地续播位置由后端 `resumeAt` 直接带回，
 * 前端只负责选集。
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
