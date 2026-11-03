/**
 * 剧集域 zod schema（与 Rust serde 模型对应）。这里是唯一的手写契约副本：
 * Rust 侧 `domain/model/*` 改了字段这里必须同步改，否则运行时才报错。
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

/** 一部剧「最近看到的那一集」（Rust `playback::SeriesProgress`）；本地 playback 表 5 秒一写，是「继续看第 N 集」的第一真值，云端历史只做没看过时的兜底。 */
export const seriesProgressSchema = z.object({
  vidIndex: z.number().int().positive(),
  currentTime: z.number().nonnegative(),
  duration: z.number().nonnegative(),
  updatedAt: z.number(),
});

/** 找剧网格的卡片（App 搜索 / 筛选流条目统一转成这个形态喂网格；纯前端形态，不走 IPC 校验）。 */
export type SeriesCard = {
  seriesId: string;
  seriesTitle: string;
  cover: string;
  episodeCount: number;
  tags: string[];
  /** 热度行（「4105万热度」；搜索结果卡有，筛选流卡片暂无） */
  heatText?: string;
  url: string;
};
