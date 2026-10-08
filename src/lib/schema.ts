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
  /** 该集评论数（detail 公开计数；旧档案缓存缺省 0 = 不显示） */
  commentCount: z.number().default(0),
  /** 该集点赞数（detail 公开计数） */
  diggCount: z.number().default(0),
  /** 该集时长（秒，选集格角标；旧档案缓存缺省 0 = 不显示） */
  duration: z.number().default(0),
});

export type Episode = z.infer<typeof episodeSchema>;

export const seriesSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  episodeCount: z.number().int().nonnegative(),
  /** 全剧收藏数（detail 公开计数；0 = 不显示） */
  followedCnt: z.number().default(0),
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

/**
 * 一部剧「最近看到的那一集」（Rust `playback::SeriesProgress`）。
 *
 * 本地 playback 表 5 秒一写，是「继续看第 N 集」的第一真值；
 * 云端观看历史（约 1 分钟一报 + 缓存）只做没看过时的兜底。
 */
export const seriesProgressSchema = z.object({
  vidIndex: z.number().int().positive(),
  currentTime: z.number().nonnegative(),
  duration: z.number().nonnegative(),
  updatedAt: z.number(),
});

export type SeriesProgress = z.infer<typeof seriesProgressSchema>;

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
  // 旧库/旧后端无此字段：宽松兜空串
  avatarUrl: z.string().catch(''),
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
  avatarUrl: z.string().catch(''),
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
/** 云端观看历史的一条（Rust `history::WatchHistoryItem`，官方 App「历史」同源）。 */
export const watchHistoryItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  /** HEIC 签名 URL，前端走 hongguo-cover 代理渲染 */
  cover: z.string(),
  vidIndex: z.number().int().nonnegative(),
  vid: z.string(),
  positionMs: z.number().nonnegative(),
  durationMs: z.number().nonnegative(),
  episodeCnt: z.number().int().nonnegative(),
  updatedAtMs: z.number(),
});

export type WatchHistoryItem = z.infer<typeof watchHistoryItemSchema>;

export const watchHistoryPageSchema = z.object({
  items: z.array(watchHistoryItemSchema),
  hasMore: z.boolean(),
  nextOffset: z.number(),
  total: z.number(),
});
export type WatchHistoryPage = z.infer<typeof watchHistoryPageSchema>;

export const storageUsageSchema = z.object({
  bytes: z.number().nonnegative(),
  files: z.number().int().nonnegative(),
});

export type StorageUsage = z.infer<typeof storageUsageSchema>;

/** 转码能力：探测「这台机器会走哪条路」。 */
export const decodeCapabilitySchema = z.object({
  hasFfmpeg: z.boolean(),
  h264HwEncoder: z.boolean(),
  /** 平台原生编码层可用（macOS：VideoToolbox 会话，含 Apple 软编；Windows：硬件 MFT） */
  platformEncoder: z.boolean(),
  /** 平台原生**硬编**——只决定徽标「硬件加速」档，不是 macOS 生产闸门 */
  platformHwEncoder: z.boolean(),
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
  /** 季角标（sub_title_list data_type=0，「第1季」；无则空串） */
  seasonTag: z.string(),
  /** 热度文本（sub_title_list data_type=27，「1705万」；无则空串） */
  heatText: z.string(),
  /** 官方运营角标（tag_info.text：「新剧/爆剧/红果首发」等；无则空串） */
  badge: z.string(),
  /** 内容类型：1=真人剧，1004=漫剧（0/未知=不过滤） */
  contentType: z.number(),
});

export const feedPageSchema = z.object({
  items: z.array(feedItemSchema),
  nextOffset: z.number(),
  hasMore: z.boolean(),
  sessionId: z.string(),
});

export type FeedItem = z.infer<typeof feedItemSchema>;
export type FeedPage = z.infer<typeof feedPageSchema>;

// ---------------------------------------------------------------- 找剧（筛选浏览）

/** 找剧筛选面板的一行（Rust `discover::SelectorRow`，rowType 即 select_items 的键）。 */
export const selectorRowSchema = z.object({
  rowType: z.string(),
  /** 服务端行名（「全部体裁」…，行头「全部」态即空选） */
  rowName: z.string(),
  items: z.array(z.object({ id: z.string(), name: z.string() })),
});

export type SelectorRow = z.infer<typeof selectorRowSchema>;

/** 找剧的八维筛选条件（空串 = 全部）。 */
export const browseFiltersSchema = z.object({
  genre: z.string().default(''),
  theme: z.string().default(''),
  role: z.string().default(''),
  epoch: z.string().default(''),
  sort: z.string().default(''),
  gender: z.string().default(''),
  onlineTime: z.string().default(''),
  duration: z.string().default(''),
});

export type BrowseFilters = z.infer<typeof browseFiltersSchema>;

/** 详情页相关作品里的一条（Rust `detail::RelatedItem`）。 */
export const relatedItemSchema = z.object({
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
});
export type SeriesMeta = z.infer<typeof seriesMetaSchema>;

/** 一条弹幕（Rust `danmaku::Danmaku` 的 camelCase 序列化）。 */
export const danmakuSchema = z.object({
  commentId: z.string(),
  text: z.string(),
  offsetMs: z.number().int().nonnegative(),
  diggCount: z.number(),
});
export type Danmaku = z.infer<typeof danmakuSchema>;

/** 一条评论区评论（Rust `danmaku::CommentItem`）。 */
export const commentItemSchema = z.object({
  commentId: z.string(),
  userName: z.string(),
  avatar: z.string(),
  text: z.string(),
  createTime: z.number(),
  diggCount: z.number(),
  replyCount: z.number(),
  userDigg: z.boolean(),
});
export type CommentItem = z.infer<typeof commentItemSchema>;

/** 评论区一页（Rust `danmaku::CommentPage`；total 是互动栏评论计数数据源）。 */
export const commentPageSchema = z.object({
  items: z.array(commentItemSchema),
  total: z.number(),
  hasMore: z.boolean(),
  nextCursor: z.string(),
});
export type CommentPage = z.infer<typeof commentPageSchema>;

/**
 * 剧级评论页 + 剧评分摘要（Rust `danmaku::SeriesReviewPage`）。
 * 评分/评分人数/题材标签在评论响应 extra 里（2026-10-07 逆向 hgplayer
 * Reviews 锁定）——详情头部的「8.0分 1074人评分」数据源。
 */
export const seriesReviewPageSchema = z.object({
  ...commentPageSchema.shape,
  /** 剧评分（"8.0"；空串 = 暂无评分） */
  score: z.string(),
  /** 评分人数 */
  scoreCnt: z.number(),
  tags: z.array(z.string()),
});
export type SeriesReviewPage = z.infer<typeof seriesReviewPageSchema>;

/** 书架（收藏）列表里的一条（Rust `interact::BookshelfEntry`）。 */
export const bookshelfEntrySchema = z.object({
  seriesId: z.string(),
  collectTimeMs: z.number(),
  /** 内容类型：1=真人，1004=漫剧（0=未知） */
  contentType: z.number(),
});
export type BookshelfEntry = z.infer<typeof bookshelfEntrySchema>;

/** 互动列表里的一条视频（Rust `interact::InteractionItem`，计数给右栏数字用）。 */
export const interactionItemSchema = z.object({
  vid: z.string(),
  seriesId: z.string(),
  userDigg: z.boolean(),
  diggedCount: z.number(),
  followed: z.boolean(),
  followedCnt: z.number(),
  seriesTitle: z.string(),
});
export type InteractionItem = z.infer<typeof interactionItemSchema>;

/** 互动状态列表（Rust `interact::InteractionState`；best-effort 回显用）。 */
export const interactionStateSchema = z.object({
  items: z.array(interactionItemSchema),
});
export type InteractionState = z.infer<typeof interactionStateSchema>;

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
  /** 子榜描述行（如 "10月4日已更新·基于红果观看/互动以及个人兴趣排序"） */
  description: z.string(),
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

/** 联想词的一个渲染片段（hl=命中高亮，Rust 按服务端命中位切好）。 */
const suggestPartSchema = z.object({
  text: z.string(),
  hl: z.boolean(),
});

/** 搜索联想条目（Rust `search::SuggestItem`，suggest/v 的 query_result_v2）。 */
export const suggestItemSchema = z.object({
  /** 联想词（= 剧名） */
  word: z.string(),
  /** 命中高亮切片；服务端没给高亮信息时为空（整体普通渲染） */
  parts: z.array(suggestPartSchema).default([]),
  /** 对应剧集 id；纯词联想为空串（前端回落为发起搜索） */
  seriesId: z.string(),
  vid: z.string(),
  cover: z.string(),
  /** 摘要行（「第1季·玄幻·4105万热度」） */
  abstract: z.string(),
});
export type SuggestItem = z.infer<typeof suggestItemSchema>;
