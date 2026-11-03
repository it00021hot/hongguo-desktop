import { call } from '../tauri/invoke';

export const series = {
  /** 分集表（选集/详情/播放器）：在线总是现拉，失败回落本地快照。 */
  episodes: (seriesId: string) => call('get_series_episodes', { seriesId }),
  /** 本地快照（列表页封面/标题）：命中零网络；未收录的剧解析一次并归档。 */
  snapshot: (seriesId: string) => call('get_series_snapshot', { seriesId }),
  resolve: (input: string) => call('resolve_series', { input }),
  /** 相关作品·系列（同系列各季 + 同 IP；失败由上层静默降级） */
  related: (seriesId: string) => call('related_series', { seriesId }),
  /** 详情页头部元信息（追剧/播放/季徽/标签/备案号；失败前端静默降级）。 */
  meta: (seriesId: string) => call('series_meta', { seriesId }),
};
