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
} as const;

export type EventName = (typeof EVENTS)[keyof typeof EVENTS];
