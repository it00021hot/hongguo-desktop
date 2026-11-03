/** 合并域 zod schema（与 Rust serde 模型对应）。 */
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

export const mergePreflightSchema = z.object({
  ok: z.boolean(),
  episodeCount: z.number().int().nonnegative(),
  estimatedSize: z.number().int().nonnegative(),
  // null 表示查不到剩余空间，跟「剩余 0 B」是两回事，不能混
  freeSpace: z.number().int().nonnegative().nullable(),
  // 快速合并是字节级顺序拼接，编码不一致会产出连索引都过不去的文件
  codecConsistent: z.boolean(),
  codecMismatchEpisode: z.number().int().positive().nullable(),
  // warnings 是 i18n key，键集由 Rust merge_service::prepare 定义（noDownloads / codecMismatch /
  // insufficientSpace）——Rust 新增警告键必须同一 commit 扩这个枚举，漏了响应校验当场暴露
  warnings: z.array(
    z.enum(['merge.noDownloads', 'merge.codecMismatch', 'merge.insufficientSpace']),
  ),
});
