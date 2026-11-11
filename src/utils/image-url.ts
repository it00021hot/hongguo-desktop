import { coverProxyUrl } from '@/utils/cover';
import { externalPageUrl } from '@/utils/offline-qa';

/**
 * 头像 CDN 地址规整（评论/回复/剧评数据层统一过这里）：http→https（打包版 CSP 与 macOS ATS 拦 http）；
 * .heic→.jpeg（tplv 模板后缀即输出格式，CDN 服务端转码）；其余无显式可渲染扩展名走 hongguo-cover 本地代理（后端魔数嗅探）。
 */
export function normalizeAvatarUrl(url: string): string {
  if (!url || externalPageUrl(url)) return '';
  let u = url.startsWith('http://') ? `https://${url.slice(7)}` : url;
  if (u.toLowerCase().endsWith('.heic')) u = `${u.slice(0, -5)}.jpeg`;
  return /\.(jpeg|jpg|png|webp)(\?|$)/i.test(u) ? u : coverProxyUrl(u);
}
