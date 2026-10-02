import { call } from './invoke';
import {
  appInfoSchema,
  browseResultSchema,
  cacheStatusSchema,
  categorySchema,
  decodeCapabilitySchema,
  downloadTaskSchema,
  mergePreflightSchema,
  mergeTaskSchema,
  onlineCacheStatusSchema,
  playbackHistoryItemSchema,
  playResponseSchema,
  proxyStatusSchema,
  proxyTestResultSchema,
  queueStatusSchema,
  seriesExtrasSchema,
  seriesSchema,
  settingsSchema,
  sniffResultSchema,
  storageUsageSchema,
  type AppInfo,
  type BrowseResult,
  type Category,
  type DecodeCapability,
  type DownloadTask,
  type MergeMode,
  type MergePreflight,
  type MergeTask,
  type PlaybackHistoryItem,
  type PlayResponse,
  type ProxyConfig,
  type ProxyStatus,
  type ProxyTestResult,
  type QueueStatus,
  type SeriesExtras,
  type Series,
  type Settings,
  type SniffResult,
  type StorageUsage,
} from '../schema';

/**
 * 全部 IPC 调用集中在这里。
 *
 * 组件不要直接 `invoke`——统一走这里才能保证 zod 校验与错误归一，
 * 也让「后端改了 command 签名」这件事在类型检查时立刻暴露。
 *
 * 参数名与 Rust 侧严格一致（Tauri 会把 JS 对象的键直接映射为 command 参数名）。
 */

// ---------------------------------------------------------------- 应用

export const app = {
  getInfo: () => call<AppInfo>('get_app_info', undefined, appInfoSchema),
  openExternalUrl: (url: string) => call<void>('open_external_url', { url }),
  selectFolder: () => call<string | null>('select_folder'),
  showInFolder: (path: string) => call<void>('show_in_folder', { path }),
  openFolder: (seriesId: string) => call<void>('open_folder', { seriesId }),
};

// ---------------------------------------------------------------- 设置

export const settings = {
  get: () => call<Settings>('get_settings', undefined, settingsSchema),
  save: (next: Settings) => call<Settings>('save_settings', { settings: next }, settingsSchema),
  proxyStatus: () => call<ProxyStatus>('get_proxy_status', undefined, proxyStatusSchema),
  testProxy: (draft?: ProxyConfig) =>
    call<ProxyTestResult>('test_proxy', { draft: draft ?? null }, proxyTestResultSchema),
  presets: () => call<[string, string][]>('proxy_presets'),
};

// ---------------------------------------------------------------- 剧集

export const series = {
  list: () => call<Series[]>('get_series_list'),
  episodes: (seriesId: string) =>
    call<Series>('get_series_episodes', { seriesId }, seriesSchema),
  resolve: (input: string) => call<Series>('resolve_series', { input }, seriesSchema),
  extras: (seriesId: string) =>
    call<SeriesExtras>('get_series_extras', { seriesId }, seriesExtrasSchema),
  remove: (seriesId: string) => call<void>('remove_series', { seriesId }),
  restoreDismissed: (seriesId: string) =>
    call<void>('restore_dismissed_series', { seriesId }),
  dismissedCount: () => call<number>('dismissed_count'),
  purgeEmpty: () => call<number>('purge_empty_series'),
};

// ---------------------------------------------------------------- 浏览与搜索

export const browse = {
  categories: () => call<Category[]>('browse_categories', undefined, categorySchema.array()),
  list: (category: string, genre: string | null, page: number) =>
    call<BrowseResult>(
      'browse_list',
      { category, genre, page },
      browseResultSchema,
    ),
};

export const search = {
  run: (keyword: string) =>
    call<SniffResult>('search_series', { keyword }, sniffResultSchema),
  setWindowVisible: (visible: boolean) =>
    call<void>('set_search_window_visible', { visible }),
};

// ---------------------------------------------------------------- 下载

export const download = {
  tasks: () => call<DownloadTask[]>('get_download_tasks', undefined, downloadTaskSchema.array()),
  status: () => call<QueueStatus>('get_queue_status', undefined, queueStatusSchema),
  start: (seriesId: string, vids: number[]) =>
    call<number>('download_batch', { seriesId, vids }),
  single: (seriesId: string, vidIndex: number) =>
    call<number>('download_single_episode', { seriesId, vidIndex }),
  stop: (taskId: string) => call<void>('stop_download', { taskId }),
  retry: (taskId: string) => call<DownloadTask>('retry_task', { taskId }, downloadTaskSchema),
  retryMany: (taskIds: string[]) => call<number>('retry_tasks', { taskIds }),
  remove: (taskIds: string[], deleteFiles = false) =>
    call<number>('delete_tasks', { taskIds, deleteFiles }),
  rescan: (seriesId: string) => call<number>('rescan_downloads', { seriesId }),
  pauseAll: () => call<number>('pause_all'),
  resumeAll: () => call<number>('resume_all'),
};

// ---------------------------------------------------------------- 合并

export const merge = {
  tasks: () => call<MergeTask[]>('get_merge_tasks', undefined, mergeTaskSchema.array()),
  preflight: (seriesId: string) =>
    call<MergePreflight>('merge_preflight', { seriesId }, mergePreflightSchema),
  start: (seriesId: string, outputName: string, mode: MergeMode) =>
    call<MergeTask>('merge_series', { seriesId, outputName, mode }, mergeTaskSchema),
  cancel: (id: string) => call<void>('cancel_merge', { id }),
  remove: (id: string) => call<void>('delete_merge_task', { id }),
};

// ---------------------------------------------------------------- 播放

export const play = {
  series: (seriesId: string, vidIndex: number, filePath = '', preferOnline = false) =>
    call<PlayResponse>(
      'play_series',
      { request: { seriesId, vidIndex, filePath, preferOnline } },
      playResponseSchema,
    ),
  // duration 必须回传：后端靠它判断「接近片尾就不续播」，不记就等于没这道防线
  savePosition: (seriesId: string, vidIndex: number, currentTime: number, duration: number) =>
    call<void>('save_playback_position', { seriesId, vidIndex, currentTime, duration }),
  getPosition: (seriesId: string, vidIndex: number) =>
    call<number>('get_playback_position', { seriesId, vidIndex }),
  history: () =>
    call<PlaybackHistoryItem[]>('get_playback_history', undefined, playbackHistoryItemSchema.array()),
};

// ---------------------------------------------------------------- 转码

export const transcode = {
  capability: () =>
    call<DecodeCapability>('decode_capability', undefined, decodeCapabilitySchema),
  compatCacheStatus: () =>
    call('compat_cache_status', undefined, cacheStatusSchema),
  clearCompatCache: () => call<number>('clear_compat_cache'),
  onlineCacheStatus: () =>
    call('online_cache_status', undefined, onlineCacheStatusSchema),
  clearOnlineCache: () => call<number>('clear_online_cache'),
};

// ---------------------------------------------------------------- 存储

export const storage = {
  usage: () => call<StorageUsage>('get_storage_usage', undefined, storageUsageSchema),
  deleteSeries: (seriesId: string) =>
    call<number>('delete_series_files', { seriesId }),
  deleteEpisode: (seriesId: string, vidIndex: number) =>
    call<boolean>('delete_episode_file', { seriesId, vidIndex }),
  deleteAll: () => call<number>('delete_all_downloaded'),
};
