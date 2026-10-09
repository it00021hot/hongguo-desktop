import { isWindows } from '@/utils/platform';

/**
 * 封面增强工具（无业务依赖的纯函数）。
 *
 * 官网 webp 兜底链已整体拆除（2026-10-08：数据面一律走官方 App 接口），
 * 封面只有「原图 / 本地代理」两条路。都失败时组件自己的 onError 占位图兜底。
 */

/**
 * HEIC 封面的本地转码代理地址（hongguo-cover 协议，后端 ffmpeg 转 JPEG）。
 *
 * URL 形态按平台：Windows 用 `http://{scheme}.localhost`（WebView2 拦截约定），
 * macOS/Linux 用 `{scheme}://localhost`（WebKit 拦真 scheme）——与 Rust 侧
 * `protocol::scheme_base` 同一套规矩，给错的表现是封面全挂。
 */
export function coverProxyUrl(remote: string): string {
  const bytes = new TextEncoder().encode(remote);
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  const b64 = btoa(bin).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
  const base = isWindows() ? 'http://hongguo-cover.localhost' : 'hongguo-cover://localhost';
  return `${base}/c/${b64}`;
}

/** WebView2 能直接渲染的封面格式（与后端 is_renderable_cover 同口径）。 */
export function isRenderableCover(url: string): boolean {
  const path = url.split('?')[0] ?? url;
  const lower = path.toLowerCase();
  if (
    lower.endsWith('.webp') ||
    lower.endsWith('.png') ||
    lower.endsWith('.jpg') ||
    lower.endsWith('.jpeg')
  ) {
    return true;
  }
  // HEIC：Windows 的 WebView2 没有 HEIF 扩展就解不了，必须走代理；
  // macOS 的 WKWebView 原生可解（系统能力，VT/ImageIO 一层），直挂原图。
  if (lower.endsWith('.heic') && navigator.platform.toUpperCase().includes('MAC')) {
    return true;
  }
  return false;
}
