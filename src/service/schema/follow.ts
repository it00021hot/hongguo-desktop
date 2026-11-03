/** 追更域 zod schema（与 Rust serde 模型对应）。 */
import { z } from 'zod';

/** 追更列表里的一部剧（Rust `interact::FollowUpdateItem`）。 */
export const followUpdateItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  /** 服务端下发 .heic 封面 URL（展示走封面转码协议） */
  cover: z.string(),
  /** 服务端记录的总集数（0 = 未下发） */
  episodeCnt: z.number(),
  /** 服务端记录的已看到第几集（0 = 没看过） */
  latestWatchedVidIndex: z.number(),
  /** true = 已完结 */
  finished: z.boolean(),
  hasWatchedLatestEpisode: z.boolean(),
  /** 追更时间（unix 秒；0 = 未下发） */
  followUpdateAt: z.number(),
});
export type FollowUpdateItem = z.infer<typeof followUpdateItemSchema>;

/** 追更列表一页（cursor 原样回传翻页；空 = 没有更多）。 */
export const followUpdatePageSchema = z.object({
  items: z.array(followUpdateItemSchema),
  cursor: z.string(),
  hasMore: z.boolean(),
});
export type FollowUpdatePage = z.infer<typeof followUpdatePageSchema>;

/** 一条更新提醒（Rust `FollowNotice`）。 */
export const followNoticeSchema = z.object({
  id: z.number(),
  /** follow = 追更更新 / online = 预约上线 */
  kind: z.string(),
  seriesId: z.string(),
  title: z.string(),
  detail: z.string(),
  /** 产生时间（unix 秒） */
  at: z.number(),
  read: z.boolean(),
});
export type FollowNotice = z.infer<typeof followNoticeSchema>;

/** 更新提醒事件载荷（Rust `FollowNoticesEvent`）。 */
export const followNoticesEventSchema = z.object({
  notices: z.array(followNoticeSchema),
});
export type FollowNoticesEvent = z.infer<typeof followNoticesEventSchema>;
