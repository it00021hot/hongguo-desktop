/**
 * 表情域 zod schema（Rust `hongguo_api::emoticon` 模型的镜像）。
 *
 * 官方表情 / 第三方 GIF / 自定义表情 / 本地图片
 * 四类共用同一套字段，差别在 `imageType` 与可选字段是否在场——
 * 第三方 GIF `imageType=2`，自定义表情与本地图片 `imageType=1`。
 */
import { z } from 'zod';

/** 表情缩略图（Rust `emoticon::EmoticonThumb`）。 */
export const emoticonThumbSchema = z.object({
  webUri: z.string(),
  dynamicUrl: z.string(),
  width: z.number(),
  height: z.number(),
  format: z.string(),
});
export type EmoticonThumb = z.infer<typeof emoticonThumbSchema>;

/** 表情条目（Rust `emoticon::Emoticon`）。 */
export const emoticonSchema = z.object({
  /** 表情 id（服务端分配）；本地图片附件为 null。 */
  id: z.string().nullable(),
  /** 静态图 URL；本地图片附件填上传接口返回的签名 URL。 */
  webUri: z.string(),
  /** 动图 URL；本地图片附件为 null。 */
  dynamicUrl: z.string().nullable(),
  /** 1 = 静态图 / 2 = 动图。 */
  imageType: z.number(),
  width: z.number(),
  height: z.number(),
  /** 图片格式（jpeg/png/webp…）；官方表情为空串。 */
  format: z.string().nullable(),
  /** 表情名；官方表情为空串。 */
  imageName: z.string().nullable(),
  thumbNails: z.array(emoticonThumbSchema).nullable(),
  /** 展开大图 URL；自定义表情有，第三方表情无。 */
  expandWebUrl: z.string().nullable(),
});
export type Emoticon = z.infer<typeof emoticonSchema>;

/** 表情列表一页（Rust `emoticon::EmoticonPage`）。 */
export const emoticonPageSchema = z.object({
  items: z.array(emoticonSchema),
  hasMore: z.boolean(),
  nextOffset: z.number(),
});
export type EmoticonPage = z.infer<typeof emoticonPageSchema>;

/** 图片上传结果（Rust `emoticon::PictureUpload`）。 */
export const pictureUploadSchema = z.object({
  /** 对象 key（`tos-cn-i-…`）。 */
  webUri: z.string(),
  /** 完整签名 URL（评论附图与收藏表情都引用它）。 */
  url: z.string(),
});
export type PictureUpload = z.infer<typeof pictureUploadSchema>;

/**
 * 选本地图片结果（Rust `emoticon_cmd::PickedImage`）。用户取消时
 * `bytes` 为空数组——选图已改走 Rust 原生文件面板（WKWebView 不弹
 * `<input type=file>` 的选择面板），取消没有独立错误形态，用空载荷表达。
 */
export const pickedImageSchema = z.object({
  bytes: z.number().array(),
  filename: z.string(),
  width: z.number(),
  height: z.number(),
  format: z.string(),
});
export type PickedImage = z.infer<typeof pickedImageSchema>;

/**
 * 收藏 / 删除表情的入参（Rust `emoticon_cmd` 的命令参数）。
 *
 * 官方形态：`image_id` 填**源表情 id**（收藏别人发的
 * 表情即填它的 id；上传本地图片时为空串），`image_url` 填该表情的
 * 静态 URL——官方收藏动图表情时传的也是静态 `web_uri`（`image_type`
 * 仍是 2）。
 */
export const emoticonCollectInputSchema = z.object({
  imageId: z.string(),
  imageUrl: z.string(),
  width: z.number(),
  height: z.number(),
  imageType: z.number().optional(),
});
export type EmoticonCollectInput = z.infer<typeof emoticonCollectInputSchema>;
