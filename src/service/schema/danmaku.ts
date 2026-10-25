/**
 * 与 Rust serde 模型对应的 zod schema（弹幕 / 评论域）。
 */
import { z } from 'zod';
import { normalizeAvatarUrl } from '@/utils/image-url';

/** 一条弹幕（Rust `danmaku::Danmaku` 的 camelCase 序列化）。 */
export const danmakuSchema = z.object({
  commentId: z.string(),
  text: z.string(),
  offsetMs: z.number().int().nonnegative(),
  diggCount: z.number(),
});
export type Danmaku = z.infer<typeof danmakuSchema>;

/** 一条评论区评论（Rust `danmaku::CommentItem`）。发送响应校验复用
 *  （comment/reply/剧评发送现在返回完整对象，commands 层要 parse）。 */
export const commentItemSchema = z.object({
  commentId: z.string(),
  /** 作者 uid（删除入口对比登录 uid 用；匿名/缺失为空串） */
  userId: z.string().default(''),
  userName: z.string(),
  /** 头像 CDN 地址规整（http→https/heic→jpeg/未知格式走本地代理） */
  avatar: z.string().transform(normalizeAvatarUrl),
  text: z.string(),
  createTime: z.number(),
  diggCount: z.number(),
  replyCount: z.number(),
  userDigg: z.boolean(),
  /** 剧评评分（expand.score，"7" 十分制字符串；单集评论恒空串） */
  score: z.string().default(''),
  /** 评分后缀文案（"观看1小时后点评"；单集评论恒空串） */
  scoreSuffixText: z.string().default(''),
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
 * 评分/评分人数/题材标签在评论响应 extra 里（2026-10-07 逆向 Reviews
 * 端点锁定）——详情头部的「8.0分 1074人评分」数据源。
 */
export const seriesReviewPageSchema = z.object({
  ...commentPageSchema.shape,
  /** 剧评分（"8.0"；空串 = 暂无评分） */
  score: z.string(),
  /** 评分人数 */
  scoreCnt: z.number(),
  tags: z.array(z.string()),
  /** 剧评标签统计（extra.filter_tag，「修仙世界观宏大 26」pill 行，2026-10-10 抓包） */
  tagStats: z.array(z.object({ tagName: z.string(), count: z.number() })).default([]),
});
export type SeriesReviewPage = z.infer<typeof seriesReviewPageSchema>;

/** 一条回复（Rust `danmaku::ReplyItem`；reply/list 2026-10-10 抓包形态）。 */
export const replyItemSchema = z.object({
  replyId: z.string(),
  /** 作者 uid（删除入口对比登录 uid 用；缺失为空串） */
  userId: z.string().default(''),
  userName: z.string(),
  /** 头像 CDN 地址规整（http→https/heic→jpeg/未知格式走本地代理） */
  avatar: z.string().transform(normalizeAvatarUrl),
  text: z.string(),
  createTime: z.number(),
  diggCount: z.number(),
  userDigg: z.boolean(),
  /** 被回复人昵称（回复评论本条时为空串） */
  replyToName: z.string().default(''),
  /** 多级回复标记：回复「回复」时是被回复那条的 replyId */
  replyToReplyId: z.string().default(''),
});
export type ReplyItem = z.infer<typeof replyItemSchema>;

/** 回复列表一页（Rust `danmaku::ReplyPage`；total 是该评论的回复总数）。 */
export const replyPageSchema = z.object({
  items: z.array(replyItemSchema),
  total: z.number(),
  hasMore: z.boolean(),
  nextCursor: z.string(),
});
export type ReplyPage = z.infer<typeof replyPageSchema>;

/** 创作者主页资料头（Rust `danmaku::CreatorProfile`，2026-10-15 官方实包）。 */
export const creatorProfileSchema = z.object({
  userId: z.string(),
  userName: z.string(),
  /** 头像 CDN 地址规整（同评论头像：heic→代理/jpeg） */
  avatar: z.string().transform(normalizeAvatarUrl),
  description: z.string().default(''),
});
export type CreatorProfile = z.infer<typeof creatorProfileSchema>;

/** 创作者主页的一部作品（Rust `danmaku::CreatorWork`；seriesId 即播放目标）。
 *  旧后端窗口期缺字段 → 各数值/数组走 default。 */
export const creatorWorkSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string().transform(normalizeAvatarUrl),
  intro: z.string().default(''),
  episodeCnt: z.number().default(0),
  playCnt: z.number().default(0),
  hot: z.number().default(0),
  tags: z.array(z.string()).default([]),
});
export type CreatorWork = z.infer<typeof creatorWorkSchema>;

/** 创作者主页作品列表一页（Rust `danmaku::CreatorWorksPage`；cursor 翻页）。 */
export const creatorWorksPageSchema = z.object({
  works: z.array(creatorWorkSchema),
  cursor: z.string().default(''),
  hasMore: z.boolean(),
  total: z.number().default(0),
});
export type CreatorWorksPage = z.infer<typeof creatorWorksPageSchema>;
