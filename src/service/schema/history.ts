/**
 * 与 Rust serde 模型对应的 zod schema（云端观看历史 / 存储域）。
 */
import { z } from 'zod';

/** 云端观看历史的一条（Rust `history::WatchHistoryItem`，官方 App「历史」同源）。 */
const watchHistoryItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  /** HEIC 签名 URL，前端走 hongguo-cover 代理渲染 */
  cover: z.string(),
  /** 观看到第几集（1 起 = 第 1 集；旧 0 基记录按第 1 集钳制展示） */
  vidIndex: z.number().int().nonnegative(),
  vid: z.string(),
  positionMs: z.number().nonnegative(),
  durationMs: z.number().nonnegative(),
  episodeCnt: z.number().int().nonnegative(),
  updatedAtMs: z.number(),
});

export type WatchHistoryItem = z.infer<typeof watchHistoryItemSchema>;

/** 本地观看历史的一行（Rust `LocalHistoryItem`）：字段与云端条目完全
 *  同形，历史页两种模式共用同一张卡片的渲染。 */
export const localHistoryItemSchema = watchHistoryItemSchema;
export type LocalHistoryItem = z.infer<typeof localHistoryItemSchema>;

export const watchHistoryPageSchema = z.object({
  items: z.array(watchHistoryItemSchema),
  hasMore: z.boolean(),
  nextOffset: z.number(),
  total: z.number(),
});

export const storageUsageSchema = z.object({
  bytes: z.number().nonnegative(),
  files: z.number().int().nonnegative(),
});

/** 按剧聚合的磁盘占用（只含磁盘上真有文件的剧，来自下载任务记录）。 */
export const storageSeriesUsageSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  bytes: z.number().nonnegative(),
  files: z.number().int().nonnegative(),
});

/** 单处缓存的占用（Rust `storage_service::cache::CachePart`）。 */
const cachePartSchema = z.object({
  files: z.number().int().nonnegative(),
  bytes: z.number().nonnegative(),
});

/** 三处缓存（在线内存流 / 兼容转码 / 封面转码）的占用总览。 */
export const cacheUsageSchema = z.object({
  online: cachePartSchema,
  compat: cachePartSchema,
  cover: cachePartSchema,
});

/** 一键清理的结果：items 含内存流分条，bytes 只算磁盘上释放的。 */
export const cacheClearResultSchema = z.object({
  items: z.number().int().nonnegative(),
  bytes: z.number().nonnegative(),
});
