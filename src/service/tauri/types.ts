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
  // 追更更新提醒（follow_service 检查出新高，载荷 = 新增通知）
  followNotices: 'follow-notices',
  // 设备注册换档完成（启动自动注册/轮换，载荷 = 新 iid）：身份敏感数据
  // （顶栏 tab、推荐流、榜单）收到后应重拉
  deviceRegistered: 'device-registered',
  // 老板键切了窗口可见性（boss_key_cmd）：true=已隐藏（前端暂停视频）
  bossKeyHidden: 'boss-key-hidden',
  // 只由 lib.rs 的 CloseRequested 拦截发出（下载事件之外的应用级事件）
  closeRequested: 'close-requested',
  // 隐身轮询器（set_incognito）发给主窗口：visible=false 暂停（只在隐身
  // 触发暂停时记「欠播」），visible=true 续播
  incognitoVisibility: 'incognito-visibility',
  // 窗口最小化检测（lib.rs Resized+is_minimized，状态翻转才发；WebView2
  // 最小化不派发 visibilitychange，前端拿不到）：true=最小化
  windowMinimize: 'window-minimize',
} as const;

export type EventName = (typeof EVENTS)[keyof typeof EVENTS];
