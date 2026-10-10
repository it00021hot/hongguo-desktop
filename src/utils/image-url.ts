/**
 * 字节系图片 CDN（`~tplv-<模板>.<格式>` 后缀即输出格式）的格式改写。
 *
 * WebView2/Chromium 没有 HEIC 解码器——评论/剧评头像里的
 * `~tplv-obj.heic` 请求 200 但解码失败，整排黑圆（hgplayer 是手机
 * App，靠系统解码所以没事）。把后缀改成 `.jpeg` 让 CDN 服务端转码：
 * 2026-10-10 curl 实证同一 URL `.heic` 返回 image/heic、`.jpeg` 返回
 * image/jpeg（.webp/.png 同理可用）。
 */
export function heicUrlToJpeg(url: string): string {
  return url.endsWith('.heic') ? `${url.slice(0, -5)}.jpeg` : url;
}
