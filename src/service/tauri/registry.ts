/**
 * IPC 命令注册表：命令名 → 入参 schema + 响应 schema。
 *
 * 与 `EVENTS` 同性质的「两端镜像」——Rust 侧命令改名/增删参数，**同一
 * commit** 改这里；漏改的表现在编译期（命令名拼错、参数键名不对、响应
 * 形状漂移都会被 tsc/zod 当场拦下），不再靠运行时撞错。
 *
 * 入参 schema 只做编译期约束不做运行时 parse（入参都是本端构造的值，
 * dev-habits #14）；响应 schema 必填——`z.literal(null)` 这类轻 schema
 * 也算 schema，禁 `z.any()`。
 */
import { z } from 'zod';
import {
  accountStateSchema,
  bookshelfEntrySchema,
  browseFiltersSchema,
  cacheClearResultSchema,
  cacheUsageSchema,
  calendarPageSchema,
  commentItemSchema,
  commentPageSchema,
  creatorProfileSchema,
  creatorWorksPageSchema,
  danmakuSchema,
  decodeCapabilitySchema,
  downloadTaskSchema,
  emoticonPageSchema,
  emoticonSchema,
  emoticonCollectInputSchema,
  feedPageSchema,
  feedTabSchema,
  followNoticeSchema,
  followUpdatePageSchema,
  hevcInstallResultSchema,
  interactionStateSchema,
  mentionUserSchema,
  loginResultSchema,
  mergeCandidateSchema,
  mergePreflightSchema,
  mergeTaskSchema,
  passportUserSchema,
  pictureUploadSchema,
  pickedImageSchema,
  playbackCapabilitySchema,
  playResponseSchema,
  proxyConfigSchema,
  proxyTestResultSchema,
  queueStatusSchema,
  localHistoryItemSchema,
  rankPageSchema,
  relatedSeriesSchema,
  replyItemSchema,
  replyPageSchema,
  searchPageSchema,
  selectorRowSchema,
  sendCodeOutcomeSchema,
  seriesMetaSchema,
  seriesProgressSchema,
  seriesReviewPageSchema,
  seriesSchema,
  settingsSchema,
  storageSeriesUsageSchema,
  storageUsageSchema,
  suggestItemSchema,
  transcodePlaybackSchema,
  watchHistoryPageSchema,
} from '../schema';

/** Rust 命令返回 `()` 序列化成 null；归一成 undefined 让调用侧当 void 用。 */
const voidResp = z.null().transform(() => undefined);

/** 入参为空（不传 / 传 undefined 等价）。 */
const noArgs = z.undefined();

/**
 * 命令 → 契约。键名 = Rust `#[tauri::command]` 函数名（snake_case，
 * dev-habits #53）。
 */
export const COMMANDS = {
  // ---- 应用 / 窗口 --------------------------------------------------
  select_folder: { args: noArgs, response: z.string().nullable() },
  open_folder: { args: z.object({ seriesId: z.string() }), response: voidResp },
  open_external_page: { args: z.object({ page: z.string() }), response: voidResp },
  enter_mini_screen: { args: noArgs, response: voidResp },
  exit_mini_screen: { args: noArgs, response: voidResp },
  set_always_on_top: { args: z.object({ enabled: z.boolean() }), response: voidResp },
  enter_portrait_screen: { args: z.object({ aspect: z.number() }), response: voidResp },
  exit_portrait_screen: { args: noArgs, response: voidResp },
  fit_portrait_screen: {
    args: z.object({ aspect: z.number(), full: z.boolean() }),
    response: voidResp,
  },
  set_portrait_full: {
    args: z.object({ aspect: z.number(), full: z.boolean() }),
    response: voidResp,
  },
  set_traffic_lights_visible: { args: z.object({ visible: z.boolean() }), response: voidResp },
  set_incognito: { args: z.object({ enabled: z.boolean() }), response: voidResp },
  /** 关闭拦截就位（app-shell 监听挂上后调用，后端才开始拦 CloseRequested） */
  mark_window_ready: { args: noArgs, response: voidResp },
  /** 退出确认后的真退出（CloseRequested 被拦后唯一的退出路径） */
  exit_app: { args: noArgs, response: voidResp },

  // ---- 解码能力 -----------------------------------------------------
  playback_capability: { args: noArgs, response: playbackCapabilitySchema },
  install_hevc_extension: { args: noArgs, response: hevcInstallResultSchema },
  decode_capability: { args: noArgs, response: decodeCapabilitySchema },
  redetect_capability: { args: noArgs, response: decodeCapabilitySchema },
  transcode_for_playback: {
    args: z.object({ seriesId: z.string(), vidIndex: z.number(), vid: z.string().optional() }),
    response: transcodePlaybackSchema,
  },

  // ---- 合并 ---------------------------------------------------------
  get_merge_tasks: { args: noArgs, response: mergeTaskSchema.array() },
  get_merge_candidates: { args: noArgs, response: mergeCandidateSchema.array() },
  merge_preflight: {
    args: z.object({ seriesId: z.string() }),
    response: mergePreflightSchema,
  },
  merge_series: {
    args: z.object({ seriesId: z.string(), outputName: z.string(), mode: z.string() }),
    response: mergeTaskSchema,
  },
  delete_merge_task: { args: z.object({ id: z.string() }), response: voidResp },
  open_merge_output: { args: z.object({ id: z.string() }), response: voidResp },

  // ---- 弹幕 / 评论 / 剧评 --------------------------------------------
  danmaku_window: {
    args: z.object({
      groupId: z.string(),
      bookId: z.string(),
      startMs: z.number(),
      durationSecs: z.number(),
      cursor: z.string(),
    }),
    response: z.object({
      items: danmakuSchema.array(),
      hasMore: z.boolean(),
      nextCursor: z.string(),
      nextStartMs: z.number(),
    }),
  },
  comment_list: {
    args: z.object({
      groupId: z.string(),
      bookId: z.string(),
      cursor: z.string().optional(),
    }),
    response: commentPageSchema,
  },
  comment_replies: {
    args: z.object({
      groupId: z.string(),
      bookId: z.string(),
      commentId: z.string(),
      cursor: z.string().optional(),
    }),
    response: replyPageSchema,
  },
  review_replies: {
    args: z.object({
      seriesId: z.string(),
      commentId: z.string(),
      cursor: z.string().optional(),
    }),
    response: replyPageSchema,
  },
  series_comment_list: {
    args: z.object({ seriesId: z.string(), cursor: z.string().optional() }),
    response: seriesReviewPageSchema,
  },
  creator_profile: {
    args: z.object({ userId: z.string() }),
    response: creatorProfileSchema,
  },
  creator_works: {
    args: z.object({ userId: z.string(), cursor: z.string().optional() }),
    response: creatorWorksPageSchema,
  },
  series_review_send: {
    args: z.object({ seriesId: z.string(), text: z.string(), score: z.number() }),
    response: commentItemSchema,
  },

  // ---- 发现（推荐流 / 找剧）------------------------------------------
  recommend_feed: {
    args: z.object({
      tab: z.string(),
      offset: z.number(),
      sessionId: z.string(),
      filterIds: z.array(z.string()),
    }),
    response: feedPageSchema,
  },
  recommend_tabs: { args: noArgs, response: feedTabSchema.array() },
  browse_panel: { args: noArgs, response: selectorRowSchema.array() },
  browse_page: {
    args: z.object({
      filters: browseFiltersSchema,
      offset: z.number(),
      sessionId: z.string(),
    }),
    response: feedPageSchema,
  },

  // ---- 下载 ---------------------------------------------------------
  get_download_tasks: { args: noArgs, response: downloadTaskSchema.array() },
  get_queue_status: { args: noArgs, response: queueStatusSchema },
  download_batch: {
    args: z.object({ seriesId: z.string(), vids: z.array(z.number()) }),
    response: z.number(),
  },
  stop_download: { args: z.object({ taskId: z.string() }), response: voidResp },
  retry_task: { args: z.object({ taskId: z.string() }), response: downloadTaskSchema },
  retry_tasks: { args: z.object({ taskIds: z.array(z.string()) }), response: z.number() },
  delete_tasks: {
    args: z.object({ taskIds: z.array(z.string()), deleteFiles: z.boolean() }),
    response: z.number(),
  },
  pause_all: { args: noArgs, response: z.number() },
  resume_all: { args: noArgs, response: z.number() },
  rescan_downloads: { args: noArgs, response: z.number() },

  // ---- 互动（官方 App API）--------------------------------------------
  danmaku_send: {
    args: z.object({
      groupId: z.string(),
      bookId: z.string(),
      text: z.string(),
      offsetMs: z.number(),
    }),
    response: z.string(),
  },
  comment_send: {
    args: z.object({
      groupId: z.string(),
      bookId: z.string(),
      text: z.string(),
      // @提及评论（可选；三字段同时在场才生效，contextCommentIds
      // 进 aibot_mention_context——官方传最近评论 id；mentionUserName
      // 是被提及者昵称，text 前缀与 rich_text 段文本都用它）
      mentionUserId: z.string().optional(),
      mentionUserName: z.string().optional(),
      contextCommentIds: z.array(z.string()).optional(),
      // 附图评论（可选；body 顶层多 image_data，
      // 表情类另带 business_param.emoji_scene_type——第三方 2 /
      // 自定义 4；本地图片评论不带该字段）
      image: emoticonSchema.nullable().optional(),
      emojiSceneType: z.number().nullable().optional(),
    }),
    response: commentItemSchema,
  },
  // ---- 表情（官方表情 / 第三方 GIF / 自定义表情）------------------------
  emoticon_list: {
    args: z.object({
      source: z.enum(['thirdParty', 'own']),
      count: z.number().optional(),
      offset: z.number().optional(),
    }),
    response: emoticonPageSchema,
  },
  emoticon_search: {
    args: z.object({
      keyword: z.string(),
      count: z.number().optional(),
      offset: z.number().optional(),
    }),
    response: emoticonPageSchema,
  },
  emoticon_upload: {
    args: z.object({ bytes: z.number().array(), filename: z.string() }),
    response: pictureUploadSchema,
  },
  emoticon_pick_image: {
    args: z.object({}),
    response: pickedImageSchema,
  },
  emoticon_collect: {
    args: emoticonCollectInputSchema,
    response: emoticonSchema,
  },
  emoticon_delete: {
    args: emoticonCollectInputSchema,
    response: z.null(),
  },
  mention_list: {
    args: z.object({ seriesId: z.string() }),
    response: mentionUserSchema.array(),
  },
  comment_reply: {
    args: z.object({
      groupId: z.string(),
      bookId: z.string(),
      replyToCommentId: z.string(),
      replyToReplyId: z.string().nullable(),
      text: z.string(),
    }),
    response: replyItemSchema,
  },
  video_digg: {
    args: z.object({ vid: z.string(), seriesId: z.string(), digg: z.boolean() }),
    response: voidResp,
  },
  comment_digg: {
    args: z.object({ commentId: z.string(), digg: z.boolean() }),
    response: voidResp,
  },
  review_digg: {
    args: z.object({ reviewId: z.string(), digg: z.boolean() }),
    response: voidResp,
  },
  review_reply_send: {
    args: z.object({
      seriesId: z.string(),
      replyToCommentId: z.string(),
      replyToReplyId: z.string().nullable(),
      text: z.string(),
    }),
    response: replyItemSchema,
  },
  comment_delete: {
    args: z.object({ commentId: z.string(), serviceId: z.union([z.literal(2), z.literal(4)]) }),
    response: voidResp,
  },
  series_collect: {
    args: z.object({ seriesId: z.string(), collect: z.boolean() }),
    response: voidResp,
  },
  series_collect_batch: {
    args: z.object({ entries: z.array(z.object({ seriesId: z.string(), collect: z.boolean() })) }),
    response: voidResp,
  },
  interaction_state: { args: noArgs, response: interactionStateSchema },
  bookshelf_list: { args: noArgs, response: bookshelfEntrySchema.array() },

  // ---- 追更 ---------------------------------------------------------
  follow_update: {
    args: z.object({ seriesId: z.string(), follow: z.boolean() }),
    response: voidResp,
  },
  follow_updates: {
    args: z.object({ cursor: z.string() }),
    response: followUpdatePageSchema,
  },
  is_following: { args: z.object({ seriesId: z.string() }), response: z.boolean() },
  follow_notices: { args: noArgs, response: followNoticeSchema.array() },
  follow_notices_read: {
    args: z.object({ ids: z.array(z.number()) }),
    response: z.number(),
  },
  follow_notices_clear: { args: noArgs, response: voidResp },
  follow_check_updates: { args: noArgs, response: z.number() },

  // ---- 登录 ---------------------------------------------------------
  login_send_code: {
    args: z.object({ mobile: z.string() }),
    response: sendCodeOutcomeSchema,
  },
  login_sms_login: {
    args: z.object({
      mobile: z.string(),
      code: z.string(),
      mfaRetryTag: z.string().nullable(),
      mfaSmsCodeKey: z.string().nullable(),
    }),
    response: loginResultSchema,
  },
  login_mfa_verify: { args: noArgs, response: loginResultSchema },
  login_mfa_cancel: { args: noArgs, response: voidResp },
  login_status: { args: noArgs, response: accountStateSchema.nullable() },
  login_user_info: { args: noArgs, response: passportUserSchema },
  login_logout: { args: noArgs, response: voidResp },

  // ---- 播放 ---------------------------------------------------------
  play_series: {
    args: z.object({
      request: z.object({
        seriesId: z.string(),
        vidIndex: z.number(),
        filePath: z.string(),
        preferOnline: z.boolean(),
        definition: z.number().nullable(),
      }),
    }),
    response: playResponseSchema,
  },
  play_prefetch: { args: z.object({ seriesId: z.string() }), response: voidResp },
  save_playback_position: {
    args: z.object({
      seriesId: z.string(),
      vidIndex: z.number(),
      currentTime: z.number(),
      duration: z.number(),
    }),
    response: voidResp,
  },
  series_progress: {
    args: z.object({ seriesId: z.string() }),
    response: seriesProgressSchema.nullable(),
  },

  // ---- 排行榜 / 新剧 / 预约 / 搜索 -------------------------------------
  rank_list: {
    args: z.object({
      selected: z.string(),
      sub: z.string(),
      panel: z.string().optional(),
      background: z.string().optional(),
      offset: z.number().optional(),
      sessionId: z.string().optional(),
      rankVersion: z.string().optional(),
    }),
    response: rankPageSchema,
  },
  new_drama_list: {
    args: z.object({ gender: z.number(), offset: z.number(), sessionId: z.string() }),
    response: rankPageSchema,
  },
  new_drama_calendar: {
    args: z
      .object({
        date: z.string().optional(),
        offset: z.number().optional(),
        sessionId: z.string().optional(),
      })
      .optional(),
    response: calendarPageSchema,
  },
  reservation_list: {
    args: z.object({
      isOnline: z.boolean(),
      offset: z.number().optional(),
      sessionId: z.string().optional(),
    }),
    response: calendarPageSchema,
  },
  reservation_reserve: {
    args: z.object({ seriesId: z.string(), reserve: z.boolean() }),
    response: voidResp,
  },
  reservations_delete: {
    args: z.object({
      itemIds: z.array(z.string()),
      allSelect: z.boolean(),
      notDelItemIds: z.array(z.string()),
      isOnline: z.boolean(),
    }),
    response: voidResp,
  },
  search_series_cmd: {
    args: z.object({
      query: z.string(),
      offset: z.number().optional(),
      searchId: z.string().optional(),
    }),
    response: searchPageSchema,
  },
  search_suggest_cmd: {
    args: z.object({ q: z.string() }),
    response: suggestItemSchema.array(),
  },

  // ---- 剧集 ---------------------------------------------------------
  get_series_episodes: { args: z.object({ seriesId: z.string() }), response: seriesSchema },
  get_series_snapshot: { args: z.object({ seriesId: z.string() }), response: seriesSchema },
  resolve_series: { args: z.object({ input: z.string() }), response: seriesSchema },
  related_series: { args: z.object({ seriesId: z.string() }), response: relatedSeriesSchema },
  series_meta: { args: z.object({ seriesId: z.string() }), response: seriesMetaSchema },

  // ---- 设置 ---------------------------------------------------------
  get_settings: { args: noArgs, response: settingsSchema },
  save_settings: {
    args: z.object({ settings: settingsSchema }),
    response: settingsSchema,
  },
  /** 缓存目录留空时的默认位置（数据目录；设置页展示用） */
  default_cache_dir: { args: noArgs, response: z.string() },
  test_proxy: {
    args: z.object({ draft: proxyConfigSchema.nullable() }),
    response: proxyTestResultSchema,
  },

  // ---- 存储 ---------------------------------------------------------
  get_storage_usage: { args: noArgs, response: storageUsageSchema },
  get_storage_series: { args: noArgs, response: storageSeriesUsageSchema.array() },
  get_cache_usage: { args: noArgs, response: cacheUsageSchema },
  clear_cache: { args: noArgs, response: cacheClearResultSchema },
  delete_series_files: { args: z.object({ seriesId: z.string() }), response: z.number() },
  delete_episode_file: {
    args: z.object({ seriesId: z.string(), vidIndex: z.number() }),
    response: z.boolean(),
  },
  delete_all_downloaded: { args: noArgs, response: z.number() },

  // ---- 云端观看历史 ---------------------------------------------------
  watch_history_list: {
    args: z.object({ offset: z.number() }),
    response: watchHistoryPageSchema,
  },
  // 记录观看历史（本地必记 + 登录态顺路上报云端，issue #10）。
  // 展示元数据必须与 Rust 命令同形嵌套在 meta 里：serde 按顶层键找
  // `meta`，平铺会被 Option 静默吞成 None（剧名/封面/总集数全落空默认），
  // 历史页整页「未知剧集」——契约表就是这道形状错配的唯一闸门。
  record_watch_history: {
    args: z.object({
      seriesId: z.string(),
      vid: z.string(),
      vidIndex: z.number(),
      positionMs: z.number(),
      durationMs: z.number(),
      meta: z.object({
        title: z.string(),
        cover: z.string(),
        episodeCnt: z.number(),
      }),
    }),
    response: voidResp,
  },
  local_history_list: { args: noArgs, response: z.array(localHistoryItemSchema) },
  local_history_delete: {
    args: z.object({ seriesIds: z.array(z.string()) }),
    response: voidResp,
  },
  watch_history_delete: {
    args: z.object({
      items: z.array(z.object({ seriesId: z.string(), vid: z.string(), vidIndex: z.number() })),
    }),
    response: voidResp,
  },
} satisfies Record<string, { args: z.ZodType; response: z.ZodType }>;

/** 命令名全集。 */
export type CommandName = keyof typeof COMMANDS;

/** 命令入参（noArgs 命令为 undefined）。 */
type CommandArgs<C extends CommandName> = z.infer<(typeof COMMANDS)[C]['args']>;

/** 命令响应（经响应 schema 校验后的形状）。 */
export type CommandResponse<C extends CommandName> = z.infer<(typeof COMMANDS)[C]['response']>;

/** 入参包：noArgs 命令可不传；其余命令必传对应形状。 */
export type CallRest<C extends CommandName> =
  CommandArgs<C> extends undefined ? [] | [undefined] : [CommandArgs<C>];
