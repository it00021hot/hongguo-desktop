/**
 * 与 Rust serde 模型对应的 zod schema（发现域：首页推荐流 / 找剧筛选）。
 */
import { z } from 'zod';

/** 信息流的一条剧集卡片（Rust `discover::FeedItem` 的 camelCase 序列化）。 */
const feedItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  horizCover: z.string(),
  vid: z.string(),
  episodeCnt: z.number().int().nonnegative(),
  playCnt: z.number(),
  commentCount: z.number(),
  score: z.number(),
  tags: z.array(z.string()),
  /** 季角标（sub_title_list data_type=0，「第1季」；无则空串） */
  seasonTag: z.string(),
  /** 热度文本（sub_title_list data_type=27，「1705万」；无则空串） */
  heatText: z.string(),
  /** 官方运营角标（tag_info.text：「新剧/爆剧/红果首发」等；无则空串） */
  badge: z.string(),
  /** 内容类型：1=真人剧，1004=漫剧（0/未知=不过滤） */
  contentType: z.number(),
});

export const feedPageSchema = z.object({
  items: z.array(feedItemSchema),
  nextOffset: z.number(),
  hasMore: z.boolean(),
  sessionId: z.string(),
});

export type FeedItem = z.infer<typeof feedItemSchema>;
export type FeedPage = z.infer<typeof feedPageSchema>;

/** 找剧筛选面板的一行（Rust `discover::SelectorRow`，rowType 即 select_items 的键）。 */
export const selectorRowSchema = z.object({
  rowType: z.string(),
  /** 服务端行名（「全部体裁」…，行头「全部」态即空选） */
  rowName: z.string(),
  items: z.array(z.object({ id: z.string(), name: z.string() })),
});

export type SelectorRow = z.infer<typeof selectorRowSchema>;

/** 找剧的八维筛选条件（空串 = 全部）。 */
export const browseFiltersSchema = z.object({
  genre: z.string().default(''),
  theme: z.string().default(''),
  role: z.string().default(''),
  epoch: z.string().default(''),
  sort: z.string().default(''),
  gender: z.string().default(''),
  onlineTime: z.string().default(''),
  duration: z.string().default(''),
});

export type BrowseFilters = z.infer<typeof browseFiltersSchema>;
