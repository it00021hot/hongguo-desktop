/**
 * 本地图片附件（评论附图 / 自定义表情共用）。照官方链路：选图只留本地 blob 预览，
 * 点发表才上传 `/reading/reader/upload/picture/`，返回的签名 URL 塞进评论 image_data。
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
 * 从 Rust 选图结果组装本地附件：字节与尺寸由 Rust 读好带回，这里只补 blob 预览；取消（bytes 空）返回 null。
 * 不用 `<input type=file>`：WKWebView 对它不弹选择面板（真实点击到达 input 也不调 runOpenPanel 委托）。
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
