import { call } from '../tauri/invoke';

export const storage = {
  usage: () => call('get_storage_usage'),
  /** 按剧聚合的占用（清理页列表，只含磁盘上真有文件的剧） */
  seriesUsage: () => call('get_storage_series'),
  cacheUsage: () => call('get_cache_usage'),
  clearCache: () => call('clear_cache'),
  deleteSeries: (seriesId: string) => call('delete_series_files', { seriesId }),
  deleteEpisode: (seriesId: string, vidIndex: number) =>
    call('delete_episode_file', { seriesId, vidIndex }),
  deleteAll: () => call('delete_all_downloaded'),
};
