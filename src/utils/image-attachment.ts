/**
 * 本地图片附件（评论附图 / 自定义表情共用）。
 *
 * 官方 App 的链路是「选图 → 发表时才上传」——实测选图后没有任何请求，
 * 点发表才打 `/reading/reader/upload/picture/`，再把返回的签名 URL 塞进
 * 评论的 `image_data`。这里照抄：附件先在本地留着（预览用 blob URL），
 * 提交时才上传。
 */

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

/** MIME → 官方 image_data.format 取值。 */
const FORMAT_BY_MIME: Record<string, string> = {
  'image/jpeg': 'jpeg',
  'image/jpg': 'jpeg',
  'image/png': 'png',
  'image/webp': 'webp',
  'image/gif': 'gif',
  'image/heic': 'heic',
  'image/heif': 'heif',
};

/** 读本地图片：字节 + 尺寸 + 格式 + 预览 URL。 */
export async function readLocalImage(file: File): Promise<LocalImageAttachment> {
  const buffer = await file.arrayBuffer();
  const bytes = Array.from(new Uint8Array(buffer));
  const previewUrl = URL.createObjectURL(file);
  const { width, height } = await imageSize(previewUrl);
  return {
    bytes,
    filename: file.name || 'image',
    width,
    height,
    format: FORMAT_BY_MIME[file.type] ?? (file.name.split('.').pop() ?? '').toLowerCase(),
    previewUrl,
  };
}

/** 取图片原始尺寸（按 EXIF 方向校正前不翻转——官方实包宽高即原始像素）。 */
function imageSize(url: string): Promise<{ width: number; height: number }> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve({ width: img.naturalWidth, height: img.naturalHeight });
    img.onerror = () => reject(new Error('读取图片失败'));
    img.src = url;
  });
}

/** 释放本地预览 URL（附件移除/发送完成后调用）。 */
export function revokePreview(url: string) {
  if (url.startsWith('blob:')) URL.revokeObjectURL(url);
}
