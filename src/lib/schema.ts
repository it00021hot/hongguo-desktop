/**
 * 与 Rust serde 模型对应的 zod schema。
 *
 * 这里是**唯一**的手写契约副本：Rust 侧 `domain/model/*` 改了字段，
 * 这里必须同步改，否则运行时才报错。类型检查的价值就在于此。
 */
import { z } from 'zod';

// ---------------------------------------------------------------- 剧集

const episodeSchema = z.object({
  vidIndex: z.number().int().positive(),
  vid: z.string(),
  title: z.string(),
  fileStem: z.string(),
});

export type Episode = z.infer<typeof episodeSchema>;

export const seriesSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  episodeCount: z.number().int().nonnegative(),
  tags: z.array(z.string()),
  episodes: z.array(episodeSchema),
  dismissed: z.boolean(),
});

export type Series = z.infer<typeof seriesSchema>;

/** 官网详情页底部的推荐短剧。 */
const recommendItemSchema = z.object({
  seriesId: z.string(),
  seriesName: z.string(),
  seriesCover: z.string(),
  episodeCount: z.number().int().nonnegative(),
});

export type RecommendItem = z.infer<typeof recommendItemSchema>;

/** 详情页附加信息：简介 + 推荐，按需取不落盘。 */
export const seriesExtrasSchema = z.object({
  intro: z.string(),
  recommendations: z.array(recommendItemSchema),
});

export type SeriesExtras = z.infer<typeof seriesExtrasSchema>;

/** 分类与题材是同一种结构（后端也合并成了一个类型），只留一份。 */
export const categorySchema = z.object({
  slug: z.string(),
  label: z.string(),
});

export type Category = z.infer<typeof categorySchema>;

const seriesCardSchema = z.object({
  seriesId: z.string(),
  seriesTitle: z.string(),
  cover: z.string(),
  episodeCount: z.number().int().nonnegative(),
  tags: z.array(z.string()),
  url: z.string(),
});

export type SeriesCard = z.infer<typeof seriesCardSchema>;

/** 浏览与搜索共用的嗅探结果结构（后端 `sniff::BrowseResult`）。 */
const browseMetaSchema = z.object({
  page: z.number().int().positive(),
  totalPages: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
  genres: z.array(categorySchema),
});

export const browseResultSchema = z.object({
  results: z.array(seriesCardSchema),
  pageTitle: z.string(),
  meta: browseMetaSchema,
});

export type BrowseResult = z.infer<typeof browseResultSchema>;

// ---------------------------------------------------------------- 下载任务

const taskStatusSchema = z.enum(['pending', 'running', 'completed', 'failed', 'stopped']);

export type TaskStatus = z.infer<typeof taskStatusSchema>;

export const downloadTaskSchema = z.object({
  id: z.string(),
  seriesId: z.string(),
  seriesTitle: z.string(),
  vidIndex: z.number().int().positive(),
  vid: z.string(),
  epTitle: z.string(),
  filePath: z.string(),
  tempPath: z.string(),
  status: taskStatusSchema,
  downloaded: z.number().nonnegative(),
  total: z.number().nonnegative(),
  error: z.string(),
  createdAt: z.number(),
  updatedAt: z.number(),
});

export type DownloadTask = z.infer<typeof downloadTaskSchema>;

export const queueStatusSchema = z.object({
  pending: z.number().int().nonnegative(),
  running: z.number().int().nonnegative(),
  completed: z.number().int().nonnegative(),
  failed: z.number().int().nonnegative(),
  limit: z.number().int().positive(),
  active: z.number().int().nonnegative(),
});

export type QueueStatus = z.infer<typeof queueStatusSchema>;

/**
 * 下载进度事件负载。
 *
 * 事件不走 zod 校验（`useEvent` 只做类型标注），所以这里只保留类型本身。
 */
export type DownloadProgress = {
  id: string;
  downloaded: number;
  total: number;
  percent: number;
};

// ---------------------------------------------------------------- 设置

const namingTemplateSchema = z.enum(['titleIndex', 'titleIndexEpisode', 'onlyTitle']);

const proxyConfigSchema = z.object({
  mode: z.enum(['system', 'manual', 'direct']),
  url: z.string(),
});

export type ProxyConfig = z.infer<typeof proxyConfigSchema>;

export const settingsSchema = z.object({
  downloadDir: z.string(),
  naming: namingTemplateSchema,
  maxConcurrency: z.number().int().min(1).max(10),
  proxy: proxyConfigSchema,
  autoDeleteAfterPlay: z.boolean(),
  autoNextEpisode: z.boolean(),
  theme: z.string(),
});

export type Settings = z.infer<typeof settingsSchema>;

export const proxyTestResultSchema = z.object({
  ok: z.boolean(),
  elapsedMs: z.number().nonnegative(),
  message: z.string(),
});

export type ProxyTestResult = z.infer<typeof proxyTestResultSchema>;

// ---------------------------------------------------------------- 合并

const mergeModeSchema = z.enum(['quick', 'compat']);
export type MergeMode = z.infer<typeof mergeModeSchema>;

export const mergeTaskSchema = z.object({
  id: z.string(),
  seriesId: z.string(),
  seriesTitle: z.string(),
  outputName: z.string(),
  mode: mergeModeSchema,
  status: z.enum(['pending', 'running', 'completed', 'failed', 'cancelled']),
  episodeCount: z.number().int().nonnegative(),
  outputPath: z.string(),
  outputSize: z.number().nonnegative(),
  percent: z.number().min(0).max(100),
  error: z.string(),
  createdAt: z.number(),
});

export type MergeTask = z.infer<typeof mergeTaskSchema>;

export const mergePreflightSchema = z.object({
  ok: z.boolean(),
  episodeCount: z.number().int().nonnegative(),
  estimatedSize: z.number().nonnegative(),
  // null 表示查不到剩余空间，跟「剩余 0 B」是两回事，不能混
  freeSpace: z.number().int().nonnegative().nullable(),
  // 快速合并是字节级顺序拼接，编码不一致会产出连索引都过不去的文件
  codecConsistent: z.boolean(),
  codecMismatchEpisode: z.number().int().positive().nullable(),
  warnings: z.array(z.string()),
});

export type MergePreflight = z.infer<typeof mergePreflightSchema>;

// ---------------------------------------------------------------- 播放与存储

export const playResponseSchema = z.object({
  url: z.string(),
  online: z.boolean(),
  resumeAt: z.number().nonnegative(),
  error: z.string(),
});

export type PlayResponse = z.infer<typeof playResponseSchema>;

/** 播放历史的一条：某部剧最近看到的一集。 */
export const playbackHistoryItemSchema = z.object({
  seriesId: z.string(),
  vidIndex: z.number().int().positive(),
  currentTime: z.number().nonnegative(),
  updatedAt: z.number(),
  // 剧名与封面由后端一并带出。历史不依赖剧集列表：把一部剧从列表移除，
  // 不该连带把它看过的记录也抹掉。
  title: z.string(),
  cover: z.string(),
});

export type PlaybackHistoryItem = z.infer<typeof playbackHistoryItemSchema>;

export const storageUsageSchema = z.object({
  bytes: z.number().nonnegative(),
  files: z.number().int().nonnegative(),
});

export type StorageUsage = z.infer<typeof storageUsageSchema>;

/** 转码能力：探测「这台机器会走哪条路」。 */
export const decodeCapabilitySchema = z.object({
  hasFfmpeg: z.boolean(),
  h264HwEncoder: z.boolean(),
});

export type DecodeCapability = z.infer<typeof decodeCapabilitySchema>;

// ---------------------------------------------------------------- 错误

/** Rust 侧 AppError 的序列化形状。 */
export const appErrorSchema = z.object({
  kind: z.string(),
  message: z.string(),
});
