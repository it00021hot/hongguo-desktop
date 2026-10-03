import { z } from 'zod';
import { call } from './invoke';
import {
  browseResultSchema,
  danmakuSchema,
  categorySchema,
  decodeCapabilitySchema,
  downloadTaskSchema,
  mergeCandidateSchema,
  mergePreflightSchema,
  mergeTaskSchema,
  playbackHistoryItemSchema,
  playResponseSchema,
  proxyTestResultSchema,
  queueStatusSchema,
  feedPageSchema,
  seriesExtrasSchema,
  seriesSchema,
  settingsSchema,
  storageUsageSchema,
  type Category,
  type Danmaku,
  type DecodeCapability,
  type DownloadTask,
  type FeedPage,
  type MergeCandidate,
  type MergeMode,
  type MergePreflight,
  type MergeTask,
  type PlaybackHistoryItem,
  type PlayResponse,
  type ProxyConfig,
  type ProxyTestResult,
  type QueueStatus,
  type Series,
  type SeriesExtras,
  type Settings,
  type BrowseResult,
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
  selectFolder: () => call<string | null>('select_folder'),
  openFolder: (seriesId: string) => call<void>('open_folder', { seriesId }),
  // 白名单网址（Rust 侧校验），设置页 ffmpeg 安装指引用
  openExternalPage: (page: string) => call<void>('open_external_page', { page }),
};

// ---------------------------------------------------------------- 设置

export const settings = {
  get: () => call<Settings>('get_settings', undefined, settingsSchema),
  save: (next: Settings) => call<Settings>('save_settings', { settings: next }, settingsSchema),
  testProxy: (draft?: ProxyConfig) =>
    call<ProxyTestResult>('test_proxy', { draft: draft ?? null }, proxyTestResultSchema),
};

// ---------------------------------------------------------------- 剧集

export const series = {
  list: () => call<Series[]>('get_series_list'),
  episodes: (seriesId: string) => call<Series>('get_series_episodes', { seriesId }, seriesSchema),
  resolve: (input: string) => call<Series>('resolve_series', { input }, seriesSchema),
  extras: (seriesId: string) =>
    call<SeriesExtras>('get_series_extras', { seriesId }, seriesExtrasSchema),
  remove: (seriesId: string) => call<void>('remove_series', { seriesId }),
  removeAll: () => call<number>('remove_all_series'),
};

// ---------------------------------------------------------------- 发现（推荐信息流）

export const discover = {
  feed: (offset?: number) =>
    call<FeedPage>('discover_feed', offset != null ? { offset } : undefined, feedPageSchema),
  webCover: (seriesId: string) =>
    call<string | null>('web_cover', { seriesId }, z.string().nullable()),
};

// ---------------------------------------------------------------- 弹幕

export const danmaku = {
  list: (groupId: string, bookId: string) =>
    call<Danmaku[]>('danmaku_list', { groupId: groupId, bookId }, danmakuSchema.array()),
};

// ---------------------------------------------------------------- 浏览与搜索

export const browse = {
  categories: () => call<Category[]>('browse_categories', undefined, categorySchema.array()),
  list: (category: string, genre: string | null, page: number) =>
    call<BrowseResult>('browse_list', { category, genre, page }, browseResultSchema),
};

export const search = {
  run: (keyword: string) => call<BrowseResult>('search_series', { keyword }, browseResultSchema),
};

// ---------------------------------------------------------------- 下载

export const download = {
  tasks: () => call<DownloadTask[]>('get_download_tasks', undefined, downloadTaskSchema.array()),
  status: () => call<QueueStatus>('get_queue_status', undefined, queueStatusSchema),
  start: (seriesId: string, vids: number[]) => call<number>('download_batch', { seriesId, vids }),
  stop: (taskId: string) => call<void>('stop_download', { taskId }),
  retry: (taskId: string) => call<DownloadTask>('retry_task', { taskId }, downloadTaskSchema),
  retryMany: (taskIds: string[]) => call<number>('retry_tasks', { taskIds }),
  remove: (taskIds: string[], deleteFiles = false) =>
    call<number>('delete_tasks', { taskIds, deleteFiles }),
  pauseAll: () => call<number>('pause_all'),
  resumeAll: () => call<number>('resume_all'),
  // 磁盘上有文件但任务记录丢了：重新登记为已完成。返回补回条数。
  rescan: () => call<number>('rescan_downloads'),
};

// ---------------------------------------------------------------- 合并

export const merge = {
  tasks: () => call<MergeTask[]>('get_merge_tasks', undefined, mergeTaskSchema.array()),
  candidates: () =>
    call<MergeCandidate[]>('get_merge_candidates', undefined, mergeCandidateSchema.array()),
  preflight: (seriesId: string) =>
    call<MergePreflight>('merge_preflight', { seriesId }, mergePreflightSchema),
  start: (seriesId: string, outputName: string, mode: MergeMode) =>
    call<MergeTask>('merge_series', { seriesId, outputName, mode }, mergeTaskSchema),
  // 只删任务记录，合并产物是独立文件，不在删除范围内
  remove: (id: string) => call<void>('delete_merge_task', { id }),
  // 定位产物文件（打开所在文件夹并选中）
  openOutput: (id: string) => call<void>('open_merge_output', { id }),
};

// ---------------------------------------------------------------- 播放

export const play = {
  series: (
    seriesId: string,
    vidIndex: number,
    filePath = '',
    preferOnline = false,
    definition?: number,
  ) =>
    call<PlayResponse>(
      'play_series',
      { request: { seriesId, vidIndex, filePath, preferOnline, definition: definition ?? null } },
      playResponseSchema,
    ),
  // duration 必须回传：后端靠它判断「接近片尾就不续播」，不记就等于没这道防线
  savePosition: (seriesId: string, vidIndex: number, currentTime: number, duration: number) =>
    call<void>('save_playback_position', { seriesId, vidIndex, currentTime, duration }),
  history: () =>
    call<PlaybackHistoryItem[]>(
      'get_playback_history',
      undefined,
      playbackHistoryItemSchema.array(),
    ),
  clearHistory: () => call<void>('clear_playback_history'),
  removeRecord: (seriesId: string) => call<void>('remove_playback_record', { seriesId }),
};

// ---------------------------------------------------------------- 转码

export const transcode = {
  capability: () => call<DecodeCapability>('decode_capability', undefined, decodeCapabilitySchema),
  redetect: () => call<DecodeCapability>('redetect_capability', undefined, decodeCapabilitySchema),
  clearCompatCache: () => call<number>('clear_compat_cache'),
  clearOnlineCache: () => call<number>('clear_online_cache'),
  transcodeForPlayback: (args: { seriesId: string; vidIndex: number; vid?: string }) =>
    call<{
      url: string;
      cached: boolean;
      backend: string;
      elapsedMs: number;
    }>('transcode_for_playback', args),
};

// ---------------------------------------------------------------- 存储

export const storage = {
  usage: () => call<StorageUsage>('get_storage_usage', undefined, storageUsageSchema),
  deleteSeries: (seriesId: string) => call<number>('delete_series_files', { seriesId }),
  deleteEpisode: (seriesId: string, vidIndex: number) =>
    call<boolean>('delete_episode_file', { seriesId, vidIndex }),
  deleteAll: () => call<number>('delete_all_downloaded'),
};
