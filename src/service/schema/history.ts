/**
 * 与 Rust serde 模型对应的 zod schema（云端观看历史 / 存储域）。
 */
import { z } from 'zod';

/** 云端观看历史的一条（Rust `history::WatchHistoryItem`，官方 App「历史」同源）。 */
export const watchHistoryItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  /** HEIC 签名 URL，前端走 hongguo-cover 代理渲染 */
  cover: z.string(),
  /** 观看到第几集（1 起 = 第 1 集；第三方写的 0 基记录按第 1 集钳制展示） */
  vidIndex: z.number().int().nonnegative(),
  vid: z.string(),
  positionMs: z.number().nonnegative(),
  durationMs: z.number().nonnegative(),
  episodeCnt: z.number().int().nonnegative(),
  updatedAtMs: z.number(),
});

export type WatchHistoryItem = z.infer<typeof watchHistoryItemSchema>;

export const watchHistoryPageSchema = z.object({
  items: z.array(watchHistoryItemSchema),
  hasMore: z.boolean(),
  nextOffset: z.number(),
  total: z.number(),
});
export type WatchHistoryPage = z.infer<typeof watchHistoryPageSchema>;

export const storageUsageSchema = z.object({
  bytes: z.number().nonnegative(),
  files: z.number().int().nonnegative(),
});

export type StorageUsage = z.infer<typeof storageUsageSchema>;

/** 按剧聚合的磁盘占用（只含磁盘上真有文件的剧，来自下载任务记录）。 */
export const storageSeriesUsageSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  bytes: z.number().nonnegative(),
  files: z.number().int().nonnegative(),
});

export type StorageSeriesUsage = z.infer<typeof storageSeriesUsageSchema>;
