/**
 * 本地图片附件（评论附图 / 自定义表情共用）。
 *
 * 官方 App 的链路是「选图 → 发表时才上传」——实测选图后没有任何请求，
 * 点发表才打 `/reading/reader/upload/picture/`，再把返回的签名 URL 塞进
 * 评论的 `image_data`。这里照抄：附件先在本地留着（预览用 blob URL），
 * 提交时才上传。
 */
import type { PickedImage } from '@/service/schema';

export interface LocalImageAttachment {
  /** 文件字节（发给 Rust 侧 multipart 上传）。 */
  bytes: number[];
  /** 文件名（multipart 的 filename 字段）。 */
  filename: string;
  width: number;
  height: number;
  /** 图片格式（jpeg/png/webp/gif…），进 image_data.format。 */
  format: string;
  /** 本地预览 URL（blob），发完即 revoke。 */
  previewUrl: string;
}

/**
 * 从 Rust 侧选图结果（`emoticon.pickImage`，原生文件面板）组装本地附件：
 * 字节与尺寸由 Rust 读好带回，这里只补 blob 预览。用户取消（`bytes`
 * 为空）返回 null。
 *
 * 为什么不直接用 `<input type=file>`：WKWebView 对它不弹选择面板
 * （真实点击到达 input 也不调 runOpenPanel 委托，2026-10-26 实测）。
 */
export function localFromPicked(picked: PickedImage): LocalImageAttachment | null {
  if (picked.bytes.length === 0) return null;
  const previewUrl = URL.createObjectURL(new Blob([new Uint8Array(picked.bytes)]));
  return {
    bytes: picked.bytes,
    filename: picked.filename,
    width: picked.width,
    height: picked.height,
    format: picked.format,
    previewUrl,
  };
}

/** 释放本地预览 URL（附件移除/发送完成后调用）。 */
export function revokePreview(url: string) {
  if (url.startsWith('blob:')) URL.revokeObjectURL(url);
}
