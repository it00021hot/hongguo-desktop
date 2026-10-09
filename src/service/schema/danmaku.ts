/**
 * 与 Rust serde 模型对应的 zod schema（弹幕 / 评论域）。
 */
import { z } from 'zod';

/** 一条弹幕（Rust `danmaku::Danmaku` 的 camelCase 序列化）。 */
export const danmakuSchema = z.object({
  commentId: z.string(),
  text: z.string(),
  offsetMs: z.number().int().nonnegative(),
  diggCount: z.number(),
});
export type Danmaku = z.infer<typeof danmakuSchema>;

/** 一条评论区评论（Rust `danmaku::CommentItem`）。 */
export const commentItemSchema = z.object({
  commentId: z.string(),
  userName: z.string(),
  avatar: z.string(),
  text: z.string(),
  createTime: z.number(),
  diggCount: z.number(),
  replyCount: z.number(),
  userDigg: z.boolean(),
});
export type CommentItem = z.infer<typeof commentItemSchema>;

/** 评论区一页（Rust `danmaku::CommentPage`；total 是互动栏评论计数数据源）。 */
export const commentPageSchema = z.object({
  items: z.array(commentItemSchema),
  total: z.number(),
  hasMore: z.boolean(),
  nextCursor: z.string(),
});
export type CommentPage = z.infer<typeof commentPageSchema>;

/**
 * 剧级评论页 + 剧评分摘要（Rust `danmaku::SeriesReviewPage`）。
 * 评分/评分人数/题材标签在评论响应 extra 里（2026-10-07 逆向 hgplayer
 * Reviews 锁定）——详情头部的「8.0分 1074人评分」数据源。
 */
export const seriesReviewPageSchema = z.object({
  ...commentPageSchema.shape,
  /** 剧评分（"8.0"；空串 = 暂无评分） */
  score: z.string(),
  /** 评分人数 */
  scoreCnt: z.number(),
  tags: z.array(z.string()),
});
export type SeriesReviewPage = z.infer<typeof seriesReviewPageSchema>;
