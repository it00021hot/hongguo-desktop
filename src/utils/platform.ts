/**
 * 运行平台判定。
 *
 * 窗口装饰自己画之后，macOS 与其余平台的差异是**布局级**的：
 * mac 的红黄绿三色按钮在左上角且是圆形，Windows 的在右上角且是方形。
 * 位置反了会直接顶到应用内容上，所以这里集中判一次，组件只消费布尔值。
 *
 * 用 UA 而不是 `navigator.userAgentData`：后者在 WebView 里并不总存在。
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

/**
 * 自定义协议 URL 是否走 `http://{scheme}.localhost` 宿主形态。
 *
 * WebView2（Windows）与 Android WebView 都不能直接请求非标准 scheme——wry
 * 靠拦截 `http://{scheme}.…` 再转回自定义协议（custom_protocol_workaround，
 * 两平台同一约定）；macOS 的 WKWebView 拦真 scheme，给宿主形态反而 404。
 * 与 Rust 侧 `protocol::scheme_base` 同一套规矩，给错的表现是封面/视频全挂。
 */
export function usesSchemeHostForm(): boolean {
  if (typeof navigator === 'undefined') return false;
  const ua = navigator.userAgent.toLowerCase();
  return ua.includes('windows') || ua.includes('android');
}
