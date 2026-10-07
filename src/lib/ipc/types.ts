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
  // 小窗销毁时由 app_cmd::open_mini_window 的事件监听发出（只发主窗）：
  // 主窗播放器据此把进度对齐到小窗刚落到后端的位置
  miniClosed: 'mini-closed',
  // 隐身轮询器（set_incognito）发给目标窗口：visible=false 时前端暂停播放
  incognitoVisibility: 'incognito-visibility',
} as const;

export type EventName = (typeof EVENTS)[keyof typeof EVENTS];
