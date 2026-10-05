/**
 * 与 Rust serde 模型对应的 zod schema。
 *
 * 这里是**唯一**的手写契约副本：Rust 侧 `domain/model/*` 改了字段，
 * 这里必须同步改，否则运行时才报错。类型检查的价值就在于此。
 */
import { z } from 'zod';

// ---------------------------------------------------------------- 剧集

const episodeSchema = z.object({
  vidIndex: z.number().int().positive(),
  vid: z.string(),
  title: z.string(),
  fileStem: z.string(),
});

export type Episode = z.infer<typeof episodeSchema>;

export const seriesSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  episodeCount: z.number().int().nonnegative(),
  tags: z.array(z.string()),
  episodes: z.array(episodeSchema),
  dismissed: z.boolean(),
});

export type Series = z.infer<typeof seriesSchema>;

/** 官网详情页底部的推荐短剧。 */
const recommendItemSchema = z.object({
  seriesId: z.string(),
  seriesName: z.string(),
  seriesCover: z.string(),
  episodeCount: z.number().int().nonnegative(),
});

export type RecommendItem = z.infer<typeof recommendItemSchema>;

/** 详情页附加信息：简介 + 推荐，按需取不落盘。 */
export const seriesExtrasSchema = z.object({
  intro: z.string(),
  recommendations: z.array(recommendItemSchema),
});

export type SeriesExtras = z.infer<typeof seriesExtrasSchema>;

/** 分类与题材是同一种结构（后端也合并成了一个类型），只留一份。 */
export const categorySchema = z.object({
  slug: z.string(),
  label: z.string(),
});

export type Category = z.infer<typeof categorySchema>;

const seriesCardSchema = z.object({
  seriesId: z.string(),
  seriesTitle: z.string(),
  cover: z.string(),
  episodeCount: z.number().int().nonnegative(),
  tags: z.array(z.string()),
  url: z.string(),
});

export type SeriesCard = z.infer<typeof seriesCardSchema>;

/** 浏览与搜索共用的嗅探结果结构（后端 `sniff::BrowseResult`）。 */
const browseMetaSchema = z.object({
  page: z.number().int().positive(),
  totalPages: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
  genres: z.array(categorySchema),
});

export const browseResultSchema = z.object({
  results: z.array(seriesCardSchema),
  pageTitle: z.string(),
  meta: browseMetaSchema,
});

export type BrowseResult = z.infer<typeof browseResultSchema>;

// ---------------------------------------------------------------- 下载任务

const taskStatusSchema = z.enum(['pending', 'running', 'completed', 'failed', 'stopped']);

export type TaskStatus = z.infer<typeof taskStatusSchema>;

export const downloadTaskSchema = z.object({
  id: z.string(),
  seriesId: z.string(),
  seriesTitle: z.string(),
  vidIndex: z.number().int().positive(),
  vid: z.string(),
  epTitle: z.string(),
  filePath: z.string(),
  tempPath: z.string(),
  status: taskStatusSchema,
  downloaded: z.number().nonnegative(),
  total: z.number().nonnegative(),
  error: z.string(),
  createdAt: z.number(),
  updatedAt: z.number(),
});

export type DownloadTask = z.infer<typeof downloadTaskSchema>;

export const queueStatusSchema = z.object({
  pending: z.number().int().nonnegative(),
  running: z.number().int().nonnegative(),
  completed: z.number().int().nonnegative(),
  failed: z.number().int().nonnegative(),
  limit: z.number().int().positive(),
  active: z.number().int().nonnegative(),
});

export type QueueStatus = z.infer<typeof queueStatusSchema>;

/**
 * 下载进度事件负载。
 *
 * 事件不走 zod 校验（`useEvent` 只做类型标注），所以这里只保留类型本身。
 */
export type DownloadProgress = {
  id: string;
  downloaded: number;
  total: number;
  percent: number;
};

// ---------------------------------------------------------------- 设置

const namingTemplateSchema = z.enum(['titleIndex', 'titleIndexEpisode', 'onlyTitle']);

const proxyConfigSchema = z.object({
  mode: z.enum(['system', 'manual', 'direct']),
  url: z.string(),
});

export type ProxyConfig = z.infer<typeof proxyConfigSchema>;

/** 已登录账号的会话快照（后端 Settings.account） */
export const accountStateSchema = z.object({
  mobile: z.string(),
  cookies: z.string(),
  userName: z.string(),
  userId: z.string(),
  loginAt: z.number(),
});

export type AccountState = z.infer<typeof accountStateSchema>;

export const settingsSchema = z.object({
  downloadDir: z.string(),
  naming: namingTemplateSchema,
  maxConcurrency: z.number().int().min(1).max(10),
  proxy: proxyConfigSchema,
  autoDeleteAfterPlay: z.boolean(),
  autoNextEpisode: z.boolean(),
  theme: z.string(),
  // 后端 serde(default)：旧库无此字段，前端宽松接收
  account: accountStateSchema.nullable().optional(),
});

export type Settings = z.infer<typeof settingsSchema>;

// ---------------------------------------------------------------- 登录

export const passportUserSchema = z.object({
  userId: z.string(),
  name: z.string(),
  mobile: z.string(),
});

export type PassportUser = z.infer<typeof passportUserSchema>;

/** 发码结果：mobileTicket 登录时必须回传 */
export const sendCodeOutcomeSchema = z.object({
  message: z.string(),
  mobileTicket: z.string(),
  /** 重发等待秒数（服务端 retry_time） */
  retryTime: z.number().default(60),
});

export type SendCodeOutcome = z.infer<typeof sendCodeOutcomeSchema>;

/** 登录命令返回：成功 / 需要 MFA 上行短信验证 / MFA 等待中 */
export const loginResultSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('success'), user: passportUserSchema }),
  z.object({
    kind: z.literal('mfa'),
    retryTag: z.string(),
    smsCodeKey: z.string(),
    /** 上行短信通道号（如 9515211003；提示文案已拼进 tips） */
    channelMobile: z.string().default(''),
    /** 要回复的短信内容（如 "YZ"） */
    smsContent: z.string().default(''),
    tips: z.string(),
  }),
  z.object({ kind: z.literal('mfaWaiting') }),
]);

export type LoginResult = z.infer<typeof loginResultSchema>;

export const proxyTestResultSchema = z.object({
  ok: z.boolean(),
  elapsedMs: z.number().nonnegative(),
  message: z.string(),
});

export type ProxyTestResult = z.infer<typeof proxyTestResultSchema>;

// ---------------------------------------------------------------- 合并

const mergeModeSchema = z.enum(['quick', 'compat']);
export type MergeMode = z.infer<typeof mergeModeSchema>;

export const mergeTaskSchema = z.object({
  id: z.string(),
  seriesId: z.string(),
  seriesTitle: z.string(),
  outputName: z.string(),
  mode: mergeModeSchema,
  status: z.enum(['pending', 'running', 'completed', 'failed', 'cancelled']),
  episodeCount: z.number().int().nonnegative(),
  outputPath: z.string(),
  outputSize: z.number().nonnegative(),
  percent: z.number().min(0).max(100),
  error: z.string(),
  createdAt: z.number(),
});

export type MergeTask = z.infer<typeof mergeTaskSchema>;

/** 可合并的剧：后端按下载队列聚合，不是剧集档案。 */
export const mergeCandidateSchema = z.object({
  seriesId: z.string(),
  seriesTitle: z.string(),
  episodeCount: z.number().int().nonnegative(),
  totalSize: z.number().nonnegative(),
});

export type MergeCandidate = z.infer<typeof mergeCandidateSchema>;

export const mergePreflightSchema = z.object({
  ok: z.boolean(),
  episodeCount: z.number().int().nonnegative(),
  estimatedSize: z.number().nonnegative(),
  // null 表示查不到剩余空间，跟「剩余 0 B」是两回事，不能混
  freeSpace: z.number().int().nonnegative().nullable(),
  // 快速合并是字节级顺序拼接，编码不一致会产出连索引都过不去的文件
  codecConsistent: z.boolean(),
  codecMismatchEpisode: z.number().int().positive().nullable(),
  warnings: z.array(z.string()),
});

export type MergePreflight = z.infer<typeof mergePreflightSchema>;

// ---------------------------------------------------------------- 播放与存储

/** 一档清晰度。 */
export const videoDefinitionSchema = z.object({
  value: z.number().int().positive(),
  width: z.number().int().nonnegative(),
  height: z.number().int().nonnegative(),
});

export type VideoDefinition = z.infer<typeof videoDefinitionSchema>;

export const playResponseSchema = z.object({
  url: z.string(),
  online: z.boolean(),
  resumeAt: z.number().nonnegative(),
  error: z.string(),
  // 实际生效的档位与本集可选的全部档位。
  // 本地文件播放时 definitions 为空数组，前端据此隐藏切换菜单。
  definition: z.number().int().nonnegative(),
  definitions: z.array(videoDefinitionSchema),
});

export type PlayResponse = z.infer<typeof playResponseSchema>;

/** 播放历史的一条：某部剧最近看到的一集。 */
export const playbackHistoryItemSchema = z.object({
  seriesId: z.string(),
  vidIndex: z.number().int().positive(),
  currentTime: z.number().nonnegative(),
  updatedAt: z.number(),
  // 剧名与封面由后端一并带出。历史不依赖剧集列表：把一部剧从列表移除，
  // 不该连带把它看过的记录也抹掉。
  title: z.string(),
  cover: z.string(),
});

export type PlaybackHistoryItem = z.infer<typeof playbackHistoryItemSchema>;

export const storageUsageSchema = z.object({
  bytes: z.number().nonnegative(),
  files: z.number().int().nonnegative(),
});

export type StorageUsage = z.infer<typeof storageUsageSchema>;

/** 转码能力：探测「这台机器会走哪条路」。 */
export const decodeCapabilitySchema = z.object({
  hasFfmpeg: z.boolean(),
  h264HwEncoder: z.boolean(),
});

/**
 * 在线播放的取流/解密进度。
 *
 * 整集取回 + 解密期间界面上原本只有一个转圈，用户既看不出在动还是卡住，
 * 也看不到还要多久。这个事件把「已收 / 总量 / 百分比 / 阶段」送上来。
 */
export const onlineProgressSchema = z.object({
  /** 缓存键，格式是 `{seriesId}:{vidIndex}` */
  key: z.string(),
  received: z.number(),
  /** CDN 没给 Content-Length 时为 0，此时不显示百分比 */
  total: z.number(),
  percent: z.number(),
  /** downloading | decrypting | ready */
  phase: z.enum(['downloading', 'decrypting', 'ready']),
});
export type OnlineProgress = z.infer<typeof onlineProgressSchema>;

/** 播放兼容兜底的转码进度。 */
export type CompatProgress = {
  /** 缓存键，格式 {seriesId}:{vidIndex} */
  key: string;
  percent: number;
  /** downloading | transcoding | ready */
  phase: 'downloading' | 'transcoding' | 'ready';
};

export type DecodeCapability = z.infer<typeof decodeCapabilitySchema>;

// ---------------------------------------------------------------- 错误

/** Rust 侧 AppError 的序列化形状。 */
export const appErrorSchema = z.object({
  kind: z.string(),
  message: z.string(),
});

// ---------------------------------------------------------------- 发现（推荐信息流）

/** 信息流的一条剧集卡片（Rust `discover::FeedItem` 的 camelCase 序列化）。 */
export const feedItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  horizCover: z.string(),
  vid: z.string(),
  episodeCnt: z.number().int().nonnegative(),
  playCnt: z.number(),
  commentCount: z.number(),
  score: z.number(),
  tags: z.array(z.string()),
});

export const feedPageSchema = z.object({
  items: z.array(feedItemSchema),
  nextOffset: z.number(),
  hasMore: z.boolean(),
  sessionId: z.string(),
});

export type FeedItem = z.infer<typeof feedItemSchema>;
export type FeedPage = z.infer<typeof feedPageSchema>;

/** 一条弹幕（Rust `danmaku::Danmaku` 的 camelCase 序列化）。 */
export const danmakuSchema = z.object({
  commentId: z.string(),
  text: z.string(),
  offsetMs: z.number().int().nonnegative(),
  diggCount: z.number(),
});
export type Danmaku = z.infer<typeof danmakuSchema>;

// ---------------------------------------------------------------- 排行榜 / 新剧 / 上新日历

/** 榜单条目（Rust `rank::RankItem`）。 */
export const rankItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  vid: z.string(),
  /** 榜单名次（从 1 起；0 表示接口没给，如预约榜） */
  rank: z.number().int().nonnegative(),
  /** "玄幻·全200集" 形态的副标题 */
  subTitle: z.string(),
  score: z.number(),
  playCnt: z.number(),
  episodeCnt: z.number().int().nonnegative(),
  /** 榜单热点文案（"13707万最高热度"） */
  recText: z.string(),
  /** 次级信息（"258.7万收藏"） */
  secondaryInfos: z.array(z.string()),
  description: z.string(),
  tags: z.array(z.string()),
});
export type RankItem = z.infer<typeof rankItemSchema>;

/** 筛选面板选项（id 为空 = 「总榜」，即清除筛选）。 */
export const rankPanelItemSchema = z.object({
  id: z.string(),
  name: z.string(),
});
export type RankPanelItem = z.infer<typeof rankPanelItemSchema>;

/** 面板一行（综合 / 时代背景 / 主题情节 / 角色设定…）。 */
export const rankPanelRowSchema = z.object({
  name: z.string(),
  items: z.array(rankPanelItemSchema),
});
export type RankPanelRow = z.infer<typeof rankPanelRowSchema>;

/** 内容 tab 下的一个子榜（自带筛选面板 schema）。 */
export const rankSubListSchema = z.object({
  id: z.string(),
  name: z.string(),
  panel: z.array(rankPanelRowSchema),
});
export type RankSubList = z.infer<typeof rankSubListSchema>;

/** 顶部内容 tab（全部/真人剧/漫剧/AI剧/系列剧；演员榜无剧集数据不提供）。 */
export const rankTabSchema = z.object({
  id: z.string(),
  name: z.string(),
  subs: z.array(rankSubListSchema),
});
export type RankTab = z.infer<typeof rankTabSchema>;

export const rankPageSchema = z.object({
  items: z.array(rankItemSchema),
  /** 内容 tab → 子榜 → 筛选面板的选项表（响应 cell_selector 展开） */
  tabs: z.array(rankTabSchema),
});
export type RankPage = z.infer<typeof rankPageSchema>;

/** 上新日历条目（Rust `rank::CalendarItem`）。 */
export const calendarItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  vid: z.string(),
  score: z.number(),
  playCnt: z.number(),
  episodeCnt: z.number().int().nonnegative(),
  description: z.string(),
  category: z.string(),
  recTags: z.array(z.string()),
  /** 排期上线时间（unix 秒；0 = 未定档） */
  publishTime: z.number(),
  isOnline: z.boolean(),
  /** 当前账号是否已预约（预约列表形态下发；日历形态恒 false） */
  hasSubscribed: z.boolean(),
});
export type CalendarItem = z.infer<typeof calendarItemSchema>;

export const calendarPageSchema = z.object({
  items: z.array(calendarItemSchema),
  /** "20261003" 形式的可选日期 */
  dates: z.array(z.string()),
  defaultDate: z.string(),
  hasMore: z.boolean(),
  nextOffset: z.number(),
  /** 预约列表两个 tab 的计数（日历形态恒 0） */
  onlineTotal: z.number(),
  offlineTotal: z.number(),
});
export type CalendarPage = z.infer<typeof calendarPageSchema>;

/** App 搜索的一条结果（Rust `search::SearchResult`）。 */
export const searchResultSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  vid: z.string(),
  subTitle: z.string(),
  score: z.number(),
  playCnt: z.number(),
  episodeCnt: z.number().int().nonnegative(),
  description: z.string(),
});
export type SearchResult = z.infer<typeof searchResultSchema>;

export const searchPageSchema = z.object({
  items: z.array(searchResultSchema),
  hasMore: z.boolean(),
  nextOffset: z.number(),
  /** 翻页会话 id（首页响应发放，翻页原样带回） */
  searchId: z.string(),
});
export type SearchPage = z.infer<typeof searchPageSchema>;
