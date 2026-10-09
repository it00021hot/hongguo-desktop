/**
 * 与 Rust serde 模型对应的 zod schema（详情域：相关作品 / 剧集元信息）。
 */
import { z } from 'zod';

/** 详情页相关作品里的一条（Rust `detail::RelatedItem`）。 */
const relatedItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  /** 角标文案（第1季/同IP/即将上线，无则空） */
  tag: z.string(),
  score: z.number(),
  playCnt: z.number(),
  /** 0 = 未上线 */
  episodeCnt: z.number().int().nonnegative(),
  videoDesc: z.string(),
});

export const relatedSeriesSchema = z.object({
  /** 相关作品·系列（同系列各季 + 同 IP） */
  works: z.array(relatedItemSchema),
  /** 猜你喜欢（可能为空，空时前端回落既有推荐源） */
  guess: z.array(relatedItemSchema),
});

export type RelatedItem = z.infer<typeof relatedItemSchema>;
export type RelatedSeries = z.infer<typeof relatedSeriesSchema>;

/** 详情页头部元信息（Rust `detail::SeriesMeta`，video_detail 接口）。 */
export const seriesMetaSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  /** 追剧数（44.7万人追剧） */
  followedCnt: z.number(),
  /** 全剧播放量（150.8万次播放） */
  playCnt: z.number(),
  /** 红果热度值（hot_score，3786万；0 = 不显示。default 兜 Rust 未重编译的窗口期） */
  hotScore: z.number().default(0),
  /** 备案号（（番茄）网微剧备字…，无则空） */
  recordNumber: z.string(),
  /** 季徽（「第1季」，secondary_infos data_type=0，无则空） */
  season: z.string(),
  /** 题材标签（玄幻/逆袭…，secondary_infos data_type=3） */
  tags: z.array(z.string()),
  /** 剧情简介（series_intro，详情页与播放器简介面板共用） */
  intro: z.string().default(''),
});
export type SeriesMeta = z.infer<typeof seriesMetaSchema>;
