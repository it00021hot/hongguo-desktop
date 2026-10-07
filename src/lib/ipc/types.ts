/**
 * 与 Rust `service/download_service/events.rs::names` 一一对应的事件名。
 *
 * 两端必须同步改：Rust 改常量名而这里没改，订阅会静默收不到事件。
 */
export const EVENTS = {
  downloadProgress: 'download-progress',
  downloadTaskAdded: 'download-task-added',
  downloadCompleted: 'download-completed',
  downloadFailed: 'download-failed',
  downloadStopped: 'download-stopped',
  downloadQueueChanged: 'download-queue-changed',
  mergeProgress: 'merge-progress',
  mergeTaskAdded: 'merge-task-added',
  mergeCompleted: 'merge-completed',
  mergeFailed: 'merge-failed',
  onlinePlayProgress: 'online-play-progress',
  loginMfaState: 'login-mfa-state',
  compatPlayProgress: 'compat-play-progress',
  seriesArchiveUpdated: 'series-archive-updated',
  // 只由 lib.rs 的 CloseRequested 拦截发出（下载事件之外的应用级事件）
  closeRequested: 'close-requested',
  // 隐身轮询器（set_incognito）发给主窗口：visible=false 暂停（只在隐身
  // 触发暂停时记「欠播」），visible=true 续播
  incognitoVisibility: 'incognito-visibility',
} as const;

export type EventName = (typeof EVENTS)[keyof typeof EVENTS];
