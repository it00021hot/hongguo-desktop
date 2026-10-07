import { z } from 'zod';
import { call } from './invoke';
import {
  accountStateSchema,
  bookshelfEntrySchema,
  browseResultSchema,
  browseFiltersSchema,
  commentPageSchema,
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
  relatedSeriesSchema,
  searchPageSchema,
  suggestItemSchema,
  selectorRowSchema,
  seriesExtrasSchema,
  seriesProgressSchema,
  seriesSchema,
  settingsSchema,
  storageUsageSchema,
  watchHistoryPageSchema,
  type AccountState,
  type BookshelfEntry,
  type BrowseFilters,
  type Category,
  type CommentPage,
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
  type RelatedSeries,
  type Series,
  type SeriesExtras,
  type SeriesProgress,
  type SelectorRow,
  type Settings,
  type BrowseResult,
  type StorageUsage,
  type CalendarPage,
  type RankPage,
  type SearchPage,
  type SuggestItem,
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
  /** 小窗播放：为某集开置顶小窗并隐藏主窗（主窗恢复由小窗关闭事件完成） */
  openMiniWindow: (seriesId: string, vidIndex: number) =>
    call<void>('open_mini_window', { seriesId, vidIndex }),
  /** 关闭小窗（返回主窗口） */
  closeMiniWindow: () => call<void>('close_mini_window'),
  /** 小窗尺寸对齐视频宽高比（起播后由小窗页调用，横屏剧=横窗零黑边） */
  fitMiniWindow: (videoWidth: number, videoHeight: number) =>
    call<void>('fit_mini_window', { videoWidth, videoHeight }),
  /**
   * 隐身模式开关：开 = 后端起系统级光标轮询（鼠标脱离窗口隐藏、回到窗口
   * 区域自动重现）；关 = 停轮询并带回可能隐藏中的窗口
   */
  setIncognito: (enabled: boolean) => call<void>('set_incognito', { enabled }),
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
  /** 相关作品·系列（同系列各季 + 同 IP；失败由上层静默降级） */
  related: (seriesId: string) =>
    call<RelatedSeries>('related_series', { seriesId }, relatedSeriesSchema),
  remove: (seriesId: string) => call<void>('remove_series', { seriesId }),
  removeAll: () => call<number>('remove_all_series'),
};

// ---------------------------------------------------------------- 发现（推荐信息流）

export const discover = {
  /** genre：'comic_series'=漫剧、'short_play'=真人剧、'ai_series'=AI剧；不传=全部 */
  feed: (offset?: number, genre?: string) =>
    call<FeedPage>('discover_feed', { offset: offset ?? 0, genre }, feedPageSchema),
  /** 找剧筛选面板（八行维度选项） */
  browsePanel: () => call<SelectorRow[]>('browse_panel', undefined, selectorRowSchema.array()),
  /** 找剧一页结果（多维筛选，服务端过滤；sessionId 首页空串、翻页回传） */
  browsePage: (filters: BrowseFilters, offset: number, sessionId = '') =>
    call<FeedPage>(
      // zod default 把 undefined 归一成空串，IPC 参数保持显式
      'browse_page',
      { filters: browseFiltersSchema.parse(filters), offset, sessionId },
      feedPageSchema,
    ),
  webCover: (seriesId: string) =>
    call<string | null>('web_cover', { seriesId }, z.string().nullable()),
};

// ---------------------------------------------------------------- 弹幕

export const danmaku = {
  list: (groupId: string, bookId: string) =>
    call<Danmaku[]>('danmaku_list', { groupId: groupId, bookId }, danmakuSchema.array()),
  /** 评论区（ct=4/src=4 形态；一窗 20 条，cursor 翻页，返回列表+总数）。 */
  comments: (groupId: string, bookId: string, cursor = '') =>
    call<CommentPage>(
      'comment_list',
      { groupId, bookId, cursor: cursor || undefined },
      commentPageSchema,
    ),
};

// ---------------------------------------------------------------- 互动（点赞 / 收藏 / 发弹幕，官方 App API）

/** 互动操作（2026-10-05/06 抓包端点；全部要求登录态，匿名被服务端静默拒）。 */
export const interact = {
  /** 发一条弹幕（offsetMs = 视频内位置毫秒），返回服务端 comment_id。 */
  sendDanmaku: (groupId: string, bookId: string, text: string, offsetMs: number) =>
    call<string>('danmaku_send', { groupId, bookId, text, offsetMs }),
  /** 发一条评论，返回 comment_id。 */
  sendComment: (groupId: string, bookId: string, text: string) =>
    call<string>('comment_send', { groupId, bookId, text }),
  /** 回复一条评论（reply/add 独立端点）；回复「回复」时传 replyToReplyId。 */
  sendReply: (
    groupId: string,
    bookId: string,
    replyToCommentId: string,
    replyToReplyId: string | null,
    text: string,
  ) =>
    call<string>('comment_reply', {
      groupId,
      bookId,
      replyToCommentId,
      replyToReplyId: replyToReplyId ?? null,
      text,
    }),
  /** 点赞 / 取消点赞一集（vid = 分集 id）。 */
  videoDigg: (vid: string, seriesId: string, digg: boolean) =>
    call<void>('video_digg', { vid, seriesId, digg }),
  /** 点赞 / 取消点赞一条评论。 */
  commentDigg: (commentId: string, digg: boolean) =>
    call<void>('comment_digg', { commentId, digg }),
  /** 收藏（追剧）/ 取消收藏一部剧。 */
  seriesCollect: (seriesId: string, collect: boolean) =>
    call<void>('series_collect', { seriesId, collect }),
  /** 最近互动列表（点赞过的 vid + 收藏的剧），回显是 best-effort 匹配。 */
  state: () => call<InteractionState>('interaction_state', undefined, interactionStateSchema),
  /** 书架（我的收藏）列表，需要登录。 */
  bookshelf: () => call<BookshelfEntry[]>('bookshelf_list', undefined, bookshelfEntrySchema.array()),
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
  /** 输入联想（失败后端已吞掉返回空表，前端无感降级） */
  suggest: (q: string) =>
    call<SuggestItem[]>('search_suggest_cmd', { q }, suggestItemSchema.array()),
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
  /** 一部剧最近看到的那一集（本地 playback 表真值；没看过返回 null） */
  progress: (seriesId: string) =>
    call<SeriesProgress | null>('series_progress', { seriesId }, seriesProgressSchema.nullable()),
};

// ---------------------------------------------------------------- 云端观看历史

/** 云端观看历史（官方 App「历史」同源；登录后可用，匿名回空表）。 */
export const watchHistory = {
  list: (offset = 0) =>
    call<WatchHistoryPage>('watch_history_list', { offset }, watchHistoryPageSchema),
  /**
   * 观看进度云上报（read_history/update + read_progress/upload 双接口）。
   * 后端匿名时静默跳过、失败只记日志——fire-and-forget 即可。
   */
  reportProgress: (seriesId: string, vid: string, vidIndex: number, positionMs: number) =>
    call<void>('cloud_report_progress', { seriesId, vid, vidIndex, positionMs }),
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
