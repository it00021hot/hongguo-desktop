/**
 * 与 Rust serde 模型对应的 zod schema（合并域）。
 */
import { z } from 'zod';

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

/** 可合并的剧：后端按下载队列聚合，不是剧集档案。 */
export const mergeCandidateSchema = z.object({
  seriesId: z.string(),
  seriesTitle: z.string(),
  episodeCount: z.number().int().nonnegative(),
  totalSize: z.number().nonnegative(),
});

export type MergeCandidate = z.infer<typeof mergeCandidateSchema>;

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
