import { externalPageUrl } from '@/utils/offline-qa';
import { usesSchemeHostForm } from '@/utils/platform';

/** 封面增强工具（无业务依赖的纯函数）：封面只有「原图 / 本地代理」两条路，都失败由组件 onError 占位图兜底。 */

/**
 * HEIC 封面的本地转码代理地址（hongguo-cover 协议，后端 ffmpeg 转 JPEG）。URL 形态按平台：
 * Windows/Android 走 `http://{scheme}.localhost`，macOS/Linux 用 `{scheme}://localhost`
 * （与 Rust 侧 `protocol::scheme_base` 同一套规矩，给错的表现是封面全挂）。
 */
export function coverProxyUrl(remote: string): string {
  const bytes = new TextEncoder().encode(remote);
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  const b64 = btoa(bin).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
  const base = usesSchemeHostForm()
    ? 'http://hongguo-cover.localhost'
    : 'hongguo-cover://localhost';
  return `${base}/c/${b64}`;
}

/** WebView2 能直接渲染的封面格式（与后端 is_renderable_cover 同口径）。 */
export function isRenderableCover(url: string): boolean {
  if (externalPageUrl(url)) return false;
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
  // HEIC：Windows 的 WebView2 没 HEIF 扩展解不了必须走代理；macOS WKWebView 原生可解，直挂原图
  if (lower.endsWith('.heic') && navigator.platform.toUpperCase().includes('MAC')) {
    return true;
  }
  return false;
}

/**
 * 封面增强：能直接渲染的格式给 undefined（组件原样加载源图）；HEIC 等
 * WebView 解不了的源给本地转码代理地址。纯同步无查询——组件渲染期直调。
 */
export function enhancedCover(source: string): string | undefined {
  if (source === '' || externalPageUrl(source)) return undefined;
  return !isRenderableCover(source) ? coverProxyUrl(source) : undefined;
}
