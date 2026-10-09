/**
 * 与 Rust serde 模型对应的 zod schema（剧集域）。
 *
 * 这里是**唯一**的手写契约副本：Rust 侧 `domain/model/*` 改了字段，
 * 这里必须同步改，否则运行时才报错。类型检查的价值就在于此。
 */
import { z } from 'zod';

const episodeSchema = z.object({
  vidIndex: z.number().int().positive(),
  vid: z.string(),
  title: z.string(),
  fileStem: z.string(),
  /** 该集评论数（detail 公开计数；旧档案缓存缺省 0 = 不显示） */
  commentCount: z.number().default(0),
  /** 该集点赞数（detail 公开计数） */
  diggCount: z.number().default(0),
  /** 该集时长（秒，选集格角标；旧档案缓存缺省 0 = 不显示） */
  duration: z.number().default(0),
});

export type Episode = z.infer<typeof episodeSchema>;

export const seriesSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  episodeCount: z.number().int().nonnegative(),
  /** 全剧收藏数（detail 公开计数；0 = 不显示） */
  followedCnt: z.number().default(0),
  tags: z.array(z.string()),
  episodes: z.array(episodeSchema),
  dismissed: z.boolean(),
});

export type Series = z.infer<typeof seriesSchema>;

/**
 * 一部剧「最近看到的那一集」（Rust `playback::SeriesProgress`）。
 *
 * 本地 playback 表 5 秒一写，是「继续看第 N 集」的第一真值；
 * 云端观看历史（约 1 分钟一报 + 缓存）只做没看过时的兜底。
 */
export const seriesProgressSchema = z.object({
  vidIndex: z.number().int().positive(),
  currentTime: z.number().nonnegative(),
  duration: z.number().nonnegative(),
  updatedAt: z.number(),
});

export type SeriesProgress = z.infer<typeof seriesProgressSchema>;

/** 找剧网格的卡片（App 搜索 / 筛选流条目统一转成这个形态喂网格；纯前端形态，不走 IPC 校验）。 */
export type SeriesCard = {
  seriesId: string;
  seriesTitle: string;
  cover: string;
  episodeCount: number;
  tags: string[];
  url: string;
};
