/**
 * 与 Rust serde 模型对应的 zod schema（搜索域）。
 */
import { z } from 'zod';

/** App 搜索的一条结果（Rust `search::SearchResult`）。 */
const searchResultSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  vid: z.string(),
  subTitle: z.string(),
  score: z.number(),
  playCnt: z.number(),
  episodeCnt: z.number().int().nonnegative(),
  description: z.string(),
  // series_sub_title_list 归一结果（旧后端可能缺：宽松兜空）
  tags: z.array(z.string()).catch([]),
  heatText: z.string().catch(''),
});
export type SearchResult = z.infer<typeof searchResultSchema>;

export const searchPageSchema = z.object({
  items: z.array(searchResultSchema),
  hasMore: z.boolean(),
  nextOffset: z.number(),
  /** 翻页会话 id（首页响应发放，翻页原样带回） */
  searchId: z.string(),
});
export type SearchPage = z.infer<typeof searchPageSchema>;

/** 联想词的一个渲染片段（hl=命中高亮，Rust 按服务端命中位切好）。 */
const suggestPartSchema = z.object({
  text: z.string(),
  hl: z.boolean(),
});

/** 搜索联想条目（Rust `search::SuggestItem`，suggest/v 的 query_result_v2）。 */
export const suggestItemSchema = z.object({
  /** 联想词（= 剧名） */
  word: z.string(),
  /** 命中高亮切片；服务端没给高亮信息时为空（整体普通渲染） */
  parts: z.array(suggestPartSchema).default([]),
  /** 对应剧集 id；纯词联想为空串（前端回落为发起搜索） */
  seriesId: z.string(),
  vid: z.string(),
  cover: z.string(),
  /** 摘要行（「第1季·玄幻·4105万热度」） */
  abstract: z.string(),
});
export type SuggestItem = z.infer<typeof suggestItemSchema>;
