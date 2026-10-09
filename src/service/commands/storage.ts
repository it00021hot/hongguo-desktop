import { call } from '../tauri/invoke';
import {
  storageSeriesUsageSchema,
  storageUsageSchema,
  type StorageSeriesUsage,
  type StorageUsage,
} from '../schema';

// ---------------------------------------------------------------- 存储

export const storage = {
  usage: () => call<StorageUsage>('get_storage_usage', undefined, storageUsageSchema),
  /** 按剧聚合的占用（清理页列表，只含磁盘上真有文件的剧） */
  seriesUsage: () =>
    call<StorageSeriesUsage[]>('get_storage_series', undefined, storageSeriesUsageSchema.array()),
  deleteSeries: (seriesId: string) => call<number>('delete_series_files', { seriesId }),
  deleteEpisode: (seriesId: string, vidIndex: number) =>
    call<boolean>('delete_episode_file', { seriesId, vidIndex }),
  deleteAll: () => call<number>('delete_all_downloaded'),
};
