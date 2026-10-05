import { z } from 'zod';
import { call } from './invoke';
import {
  accountStateSchema,
  browseResultSchema,
  commentItemSchema,
  danmakuSchema,
  categorySchema,
  decodeCapabilitySchema,
  downloadTaskSchema,
  interactionStateSchema,
  loginResultSchema,
  sendCodeOutcomeSchema,
  mergeCandidateSchema,
  mergePreflightSchema,
  mergeTaskSchema,
  passportUserSchema,
  playResponseSchema,
  proxyTestResultSchema,
  queueStatusSchema,
  calendarPageSchema,
  feedPageSchema,
  rankPageSchema,
  searchPageSchema,
  seriesExtrasSchema,
  seriesSchema,
  settingsSchema,
  storageUsageSchema,
  watchHistoryPageSchema,
  type AccountState,
  type Category,
  type CommentItem,
  type Danmaku,
  type DecodeCapability,
  type DownloadTask,
  type InteractionState,
  type FeedPage,
  type LoginResult,
  type SendCodeOutcome,
  type MergeCandidate,
  type MergeMode,
  type MergePreflight,
  type MergeTask,
  type PassportUser,
  type PlayResponse,
  type ProxyConfig,
  type ProxyTestResult,
  type QueueStatus,
  type Series,
  type SeriesExtras,
  type Settings,
  type BrowseResult,
  type StorageUsage,
  type CalendarPage,
  type RankPage,
  type SearchPage,
  type WatchHistoryPage,
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

// ---------------------------------------------------------------- 登录

export const login = {
  sendCode: (mobile: string) =>
    call<SendCodeOutcome>('login_send_code', { mobile }, sendCodeOutcomeSchema),
  smsLogin: (
    mobile: string,
    code: string,
    ticket?: string,
    mfa?: { retryTag: string; smsCodeKey: string },
  ) =>
    call<LoginResult>(
      'login_sms_login',
      {
        mobile,
        code,
        mobileTicket: ticket ?? null,
        mfaRetryTag: mfa?.retryTag ?? null,
        mfaSmsCodeKey: mfa?.smsCodeKey ?? null,
      },
      loginResultSchema,
    ),
  mfaVerify: () => call<LoginResult>('login_mfa_verify', undefined, loginResultSchema),
  /** 取消进行中的 MFA 验证（后台轮询随之停止） */
  mfaCancel: () => call<void>('login_mfa_cancel', undefined),
  status: () => call<AccountState | null>('login_status', undefined, accountStateSchema.nullable()),
  userInfo: () => call<PassportUser>('login_user_info', undefined, passportUserSchema),
  logout: () => call<void>('login_logout'),
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
  /** 评论区（ct=4/src=4 形态，与弹幕同端点）。 */
  comments: (groupId: string, bookId: string) =>
    call<CommentItem[]>('comment_list', { groupId, bookId }, commentItemSchema.array()),
};

// ---------------------------------------------------------------- 互动（点赞 / 收藏 / 发弹幕，官方 App API）

/** 互动操作（2026-10-05 抓包端点；全部要求登录态，匿名被服务端静默拒）。 */
export const interact = {
  /** 发一条弹幕（offsetMs = 视频内位置毫秒），返回服务端 comment_id。 */
  sendDanmaku: (groupId: string, bookId: string, text: string, offsetMs: number) =>
    call<string>('danmaku_send', { groupId, bookId, text, offsetMs }),
  /** 发一条评论（评论区 UI 预留）。 */
  sendComment: (groupId: string, bookId: string, text: string) =>
    call<string>('comment_send', { groupId, bookId, text }),
  /** 点赞 / 取消点赞一集（vid = 分集 id）。 */
  videoDigg: (vid: string, seriesId: string, digg: boolean) =>
    call<void>('video_digg', { vid, seriesId, digg }),
  /** 点赞 / 取消点赞一条评论（评论区 UI 预留）。 */
  commentDigg: (commentId: string, digg: boolean) =>
    call<void>('comment_digg', { commentId, digg }),
  /** 收藏（追剧）/ 取消收藏一部剧。 */
  seriesCollect: (seriesId: string, collect: boolean) =>
    call<void>('series_collect', { seriesId, collect }),
  /** 最近互动列表（点赞过的 vid + 收藏的剧），回显是 best-effort 匹配。 */
  state: () => call<InteractionState>('interaction_state', undefined, interactionStateSchema),
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

// ---------------------------------------------------------------- 排行榜 / 新剧 / 搜索（官方 App API）

export const rank = {
  /**
   * 拉一个榜单（任意 内容tab × 子榜 × 筛选 组合）。
   * selected/sub 用响应 tabs schema 下发的 id；panel 为空串 = 总榜（无筛选）。
   */
  list: (selected: string, sub: string, panel: string = '') =>
    call<RankPage>(
      'rank_list',
      { selected, sub, panel: panel === '' ? undefined : panel },
      rankPageSchema,
    ),
  /** 新剧推荐（gender: 2=全部；offset 步长 18）。 */
  newDrama: (gender: number, offset?: number) =>
    call<RankPage>('new_drama_list', { gender, offset: offset ?? 0 }, rankPageSchema),
  /** 上新日历（date 传返回值 dates 里的日期，不传取默认日）。 */
  calendar: (date?: string) =>
    call<CalendarPage>(
      'new_drama_calendar',
      date != null ? { date } : undefined,
      calendarPageSchema,
    ),
  /** 我的预约（匿名通常空表；登录后条目带 hasSubscribed）。 */
  reservations: (isOnline = true) =>
    call<CalendarPage>('reservation_list', { isOnline }, calendarPageSchema),
  /** 预约 / 取消预约（需要登录）。 */
  reserve: (seriesId: string, reserve = true) =>
    call<void>('reservation_reserve', { seriesId, reserve }),
};

/** 官方 App 搜索（综合 tab，首页是精选少数，翻页才是完整列表）。 */
export const seriesSearch = {
  run: (query: string, offset?: number, searchId?: string) =>
    call<SearchPage>(
      'search_series_cmd',
      offset != null && offset > 0 ? { query, offset, searchId: searchId ?? '' } : { query },
      searchPageSchema,
    ),
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
  /** 预取一部剧第 1 集的在线流（沉浸流「一切就下一部」）。幂等且静默。 */
  prefetch: (seriesId: string) => call<void>('play_prefetch', { seriesId }),
  // duration 必须回传：后端靠它判断「接近片尾就不续播」，不记就等于没这道防线
  savePosition: (seriesId: string, vidIndex: number, currentTime: number, duration: number) =>
    call<void>('save_playback_position', { seriesId, vidIndex, currentTime, duration }),
};

// ---------------------------------------------------------------- 云端观看历史

/** 云端观看历史（官方 App「历史」同源；登录后可用，匿名回空表）。 */
export const watchHistory = {
  list: (offset = 0) =>
    call<WatchHistoryPage>('watch_history_list', { offset }, watchHistoryPageSchema),
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
