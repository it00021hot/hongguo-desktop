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

/** Windows 判定（自定义协议 URL 形态要用：WebView2 走 http://{scheme}.localhost）。 */
export function isWindows(): boolean {
  if (typeof navigator === 'undefined') return false;
  return navigator.userAgent.toLowerCase().includes('windows');
}
