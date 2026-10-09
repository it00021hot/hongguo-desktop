/**
 * 与 Rust serde 模型对应的 zod schema（互动域：点赞 / 收藏 / 书架）。
 */
import { z } from 'zod';

/** 书架（收藏）列表里的一条（Rust `interact::BookshelfEntry`）。 */
export const bookshelfEntrySchema = z.object({
  seriesId: z.string(),
  collectTimeMs: z.number(),
  /** 内容类型：1=真人，1004=漫剧（0=未知） */
  contentType: z.number(),
});
export type BookshelfEntry = z.infer<typeof bookshelfEntrySchema>;

/** 互动列表里的一条视频（Rust `interact::InteractionItem`，计数给右栏数字用）。 */
export const interactionItemSchema = z.object({
  vid: z.string(),
  seriesId: z.string(),
  userDigg: z.boolean(),
  diggedCount: z.number(),
  followed: z.boolean(),
  followedCnt: z.number(),
  seriesTitle: z.string(),
});
export type InteractionItem = z.infer<typeof interactionItemSchema>;

/** 互动状态列表（Rust `interact::InteractionState`；best-effort 回显用）。 */
export const interactionStateSchema = z.object({
  items: z.array(interactionItemSchema),
});
export type InteractionState = z.infer<typeof interactionStateSchema>;
