/**
 * 与 Rust serde 模型对应的 zod schema（下载任务域）。
 */
import { z } from 'zod';

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
