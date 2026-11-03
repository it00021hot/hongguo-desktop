/** 下载任务域 zod schema（与 Rust serde 模型对应）。 */
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

/** 下载进度事件负载（Rust scheduler 按任务节流上报）。 */
export const downloadProgressSchema = z.object({
  id: z.string(),
  downloaded: z.number(),
  total: z.number(),
  percent: z.number(),
});

export type DownloadProgress = z.infer<typeof downloadProgressSchema>;
