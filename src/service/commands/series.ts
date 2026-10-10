import { call } from '../tauri/invoke';
import {
  relatedSeriesSchema,
  seriesMetaSchema,
  seriesSchema,
  type RelatedSeries,
  type Series,
  type SeriesMeta,
} from '../schema';

// ---------------------------------------------------------------- 剧集

export const series = {
  episodes: (seriesId: string) => call<Series>('get_series_episodes', { seriesId }, seriesSchema),
  resolve: (input: string) => call<Series>('resolve_series', { input }, seriesSchema),
  /** 相关作品·系列（同系列各季 + 同 IP；失败由上层静默降级） */
  related: (seriesId: string) =>
    call<RelatedSeries>('related_series', { seriesId }, relatedSeriesSchema),
  /** 详情页头部元信息（追剧/播放/季徽/标签/备案号；失败前端静默降级）。 */
  meta: (seriesId: string) => call<SeriesMeta>('series_meta', { seriesId }, seriesMetaSchema),
};
