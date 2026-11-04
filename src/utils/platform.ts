/**
 * 运行平台判定。窗口装饰自己画之后，mac（红黄绿在左上、圆形）与其余平台（右上、方形）是布局级差异，
 * 集中判一次组件只消费布尔值。用 UA 而不是 `navigator.userAgentData`（WebView 里并不总存在）。
 */

export function isMac(): boolean {
  if (typeof navigator === 'undefined') return false;
  return navigator.userAgent.toLowerCase().includes('mac');
}

/** Windows 判定（窗口装饰布局用）。 */
export function isWindows(): boolean {
  if (typeof navigator === 'undefined') return false;
  return navigator.userAgent.toLowerCase().includes('windows');
}

/** Android WebView（无系统 HEVC 扩展可装，不可解即自动兼容转码）。 */
export function isAndroid(): boolean {
  if (typeof navigator === 'undefined') return false;
  return navigator.userAgent.toLowerCase().includes('android');
}

/**
 * 自定义协议 URL 是否走 `http://{scheme}.localhost` 宿主形态：WebView2 与 Android WebView
 * 不能直接请求非标准 scheme（wry 拦 http 再转回），macOS WKWebView 拦真 scheme、给宿主形态反而 404
 * ——与 Rust 侧 `protocol::scheme_base` 同一套规矩，给错的表现是封面/视频全挂。
 */
export function usesSchemeHostForm(): boolean {
  if (typeof navigator === 'undefined') return false;
  const ua = navigator.userAgent.toLowerCase();
  return ua.includes('windows') || ua.includes('android');
}
