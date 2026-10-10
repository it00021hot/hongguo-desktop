import { coverProxyUrl } from '@/utils/cover';

/**
 * 头像 CDN 地址规整（评论/回复/剧评数据层统一过这里）。
 *
 * 真实地址形态繁多（2026-10-10 抓样 20 条：http 签名 jpeg / https jpeg /
 * png 模板 / 无扩展名哈希 / .heic 五种），单一「换后缀」覆盖不了：
 *
 * 1. `http://` → `https://`：fqnovelpic 签名 URL 同签名 https 直接 200
 *    （curl 实证）；打包版 CSP `img-src` 没有 `http:`、macOS ATS 也拦 http。
 * 2. `.heic` 后缀 → `.jpeg`：tplv 模板后缀即输出格式，CDN 服务端转码，
 *    WebView2 实测可解（裸 heic 请求 200 但无解码器）。
 * 3. 其余没有显式可渲染扩展名的（无扩展名哈希、passport 的 `.image` 等）→
 *    hongguo-cover 本地代理：后端魔数嗅探，jpeg/png/webp 原样透传，
 *    真是 HEIC 走既有转码阶梯，产物落盘缓存。
 */
export function normalizeAvatarUrl(url: string): string {
  if (!url) return url;
  let u = url.startsWith('http://') ? `https://${url.slice(7)}` : url;
  if (u.toLowerCase().endsWith('.heic')) u = `${u.slice(0, -5)}.jpeg`;
  return /\.(jpeg|jpg|png|webp)(\?|$)/i.test(u) ? u : coverProxyUrl(u);
}
