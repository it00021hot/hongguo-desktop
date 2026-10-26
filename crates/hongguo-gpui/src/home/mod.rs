//! 首页：沉浸式播放流（Tauri `home-page.tsx` 的 GPUI 对齐实现）。
//!
//! 视频铺满整页：滚轮在当前 tab 源内切上一部/下一部剧，剧名/集数/热度叠加
//! 在画面左下，切剧间隙封面占位。顶栏 tab 列表由服务端下发（bookmall/tab
//! 响应 tab_item[]，回落三件套推荐 16 / 漫剧 36 / 真人 39），同一接口供数，
//! **重复点当前 tab = 换一批**（重开会话、游标归零）；推荐 tab 把「你的
//! 内容」（云端历史 + 书架）种子排前，同一个号打开就是你的剧（对齐官方
//! 客户端与 Tauri 端）。
//!
//! 播放经系统播放器（macOS AVPlayer / Windows MediaEngine，经
//! `hongguo_player` 门面）：起点决策（本地档案 >3 秒续播，云端历史兜底）、
//! 5 秒节流落盘、首帧出画预取下一部的流、自动连播（末集接流内下一部——
//! Tauri 的剧终接力跨季/换批语义后续对齐）。取流编排与 Tauri 同一段
//! service（运行时共用，见 `hongguo_service::runtime`）。
//!
//! 控制区（对齐 Tauri 版控制栏的核心件）：暂停/继续（按钮 + 点击画面）、
//! 进度条拖动 seek、倍速菜单（0.75–3x，音频变速不变调）、音量/静音；偏好
//! 经 [`crate::playback_prefs`] 跨集跨启动保留。键盘快捷键（空格/方向键）
//! 与 ↑↓ 切剧随控制层下一批目标落地；「回首页 5 分钟换一批」的过期重开
//! 已对齐（FEED_STALE 判定 + pin_hold 在播不动）。
//!
//! 本文件只留视图本体（HomeView/NowPlaying 状态、构造、订阅、播放器事件
//! 循环与 Render 入口）；按域拆出的子模块：`feed`（取流/展示列表）、
//! `playback`（播放编排）、`danmaku`（弹幕）、`comments`（评论/回复）、
//! `interact`（点赞/收藏/互动栏）、`auth`（登录/头像）、`controls`（控制条
//! 与进度轨道）、`mini_panel`（小屏）、`picker`(选集浮层)、`downloads`
//! （下载面板）、`stage`（舞台/叠加层渲染）、`fmt`（格式化）。

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::SliderState;
use gpui_kit::prelude::*;
use gpui_kit::{Entity, Window};
use hongguo_api::danmaku::{CommentItem, Danmaku, ReplyItem};
use hongguo_api::detail::RelatedItem;
use hongguo_api::history::WatchHistoryItem;
use hongguo_api::interact::InteractionItem;
use hongguo_common::model::{DownloadTask, VideoDefinition};
use hongguo_service::app_state::AppStateInner;

use danmaku::DmFlight;
pub(crate) use feed::Source;
use feed::{SourceFeed, StreamItem};
use history_cadence::HistoryCadence;
use stage::StageCover;

/// 首页发往壳的事件（页面间不互相持有，壳订阅后裁决目的地）。
#[derive(Debug, Clone)]
pub(crate) enum HomeEvent {
    /// 点剧名 → 打开应用内详情页。
    OpenDetail(String),
    /// 登录/登出成功：壳刷新头像与追更未读角标（账号态变化的唯一推送口）。
    AccountChanged,
    /// 互动登录墙：切到设置页（账号卡登录入口；Tauri requireLogin 同款）。
    OpenSettings,
    /// 下载提交成功：切下载任务页（Tauri `navigate('/tasks')` 同款）。
    OpenTasks,
    /// chrome（顶栏/侧栏）显隐变了：无边框/沉浸全屏切换后壳必须重绘。
    /// 壳的 `Shell::render` 只在自己被通知时重读 `immersive_fullscreen()`；
    /// 不推这一条的话，无边框点下去顶栏不动、再点回来也不动——用户看到的
    /// 就是「按钮状态一直不对」（2026-10-18 实测）。
    ChromeChanged,
}

pub(crate) mod auth;
pub(crate) mod comment_view;
pub(crate) mod comments;
pub(crate) mod controls;
pub(crate) mod danmaku;
pub(crate) mod downloads;
pub(crate) mod downloads_range;
pub(crate) mod events;
pub(crate) mod feed;
pub(crate) mod fmt;
pub(crate) mod hevc_prompt;
pub(crate) mod history_cadence;
pub(crate) mod incognito;
pub(crate) mod interact;
pub(crate) mod mini_panel;
pub(crate) mod minimize;
pub(crate) mod overlay;
pub(crate) mod picker;
pub(crate) mod playback;
pub(crate) mod player;
pub(crate) mod player_icons;
pub(crate) mod portrait_panel;
#[cfg(feature = "screenshot")]
pub(crate) mod preview_seed;
pub(crate) mod season;
pub(crate) mod skin_focus;
pub(crate) mod stage;
pub(crate) mod state;
pub(crate) mod view;
pub(crate) mod window;

/// 当前在播的剧（overlay 与进度落盘的身份）。
///
/// 展示标记（热度/季角标/运营角标）在**起播时**从流条目上抄一份：档案接口
/// 没有这几个字段，而叠加层必须跟着「在播的那部」走——跟游标走就会在换批/
/// 钉住条目位移时错位到相邻剧（Tauri 的 overlayMeta 也是随目标传下去的）。
#[derive(Debug, Clone)]
struct NowPlaying {
    series_id: String,
    title: String,
    vid_index: u32,
    episode_count: usize,
    heat_text: String,
    season_tag: String,
    badge: String,
    /// 本集提供的全部档位（清晰度菜单数据源；本地集为空 = 菜单置灰）
    definitions: Vec<VideoDefinition>,
    /// 实际在播的档位（0 = 尚未取到）
    definition: u32,
    /// 本集的 vid（弹幕 group_id）
    vid: String,
    /// 封面 URL（观看历史记录用；直播流条目自带，档案回填的流可能为空）
    cover: String,
    /// 本剧全部分集（集号 → vid）：预取下一集与片尾接力都从这里取 vid。
    ///
    /// 缓存在这里是为了让**主线程不碰库**——`Store` 是同步阻塞门面
    /// （投递 DB 线程后等结果），切集路径上每集读一次库就是主线程上多一次
    /// 跨线程往返。起播时档案已经在手（`play_task` 读的），顺手带下来。
    episode_vids: Arc<Vec<(u32, String)>>,
}

// ---------------------------------------------------------------- 视图

pub struct HomeView {
    state: Arc<AppStateInner>,
    active: Source,
    feeds: HashMap<Source, SourceFeed>,
    indexes: HashMap<Source, usize>,
    /// 顶栏 tab（服务端下发，回落三件套初始化；stage 渲染与壳导航用）。
    tabs: Vec<Source>,
    /// 服务端下发的 tab 标题（tab_type → title；缺省回落本地名）。
    tab_labels: HashMap<String, String>,

    /// 全屏占位用的**原图档**封面（series_id → 状态，见
    /// [`StageCover`]）：只有当前/目标这一集，铺满舞台要原图（列表卡档是
    /// 缩到 320 长边的小图，不够铺）。
    stage_covers: HashMap<String, StageCover>,
    // —— 播放态 ——
    /// 播放纪元：每次切剧 +1，陈旧事件/任务按它丢弃。Arc 原子共享给
    /// 事件循环——事件在后台线程收取，主线程字段够不着就要共享。
    epoch: Arc<AtomicU64>,
    player: Option<player::Player>,
    /// 预览的虚拟播放器标记（screenshot feature）：预览无真实播放器，而控制条/
    /// 选集/下载面板的渲染门槛都是 player.is_some()——种子置位后同一门槛
    /// 放行，这些 UI 走同一段渲染代码出画（见 [`Self::engine_ready`]）。
    #[cfg(feature = "screenshot")]
    preview_engine: bool,
    now: Option<NowPlaying>,
    /// 「在播不动」闸门（Tauri pinHold 同口径）：换一批（同 tab 重开）开闸
    /// ——在播剧被换出列表时钉在最前，新批到位游标 0 仍是它，不切；跨 tab
    /// 切换关闸——新源首批到位即换游标处的剧（切 tab 就换片）。不关闸的话
    /// 钉子会占住新源的第 0 位，播放目标判「就是在播」而不切（点了没反应
    /// 的根因）。HomeView 实体常驻：信息流/游标/封面缓存跨目的地保留；
    /// 播放器生命周期不保活——离开首页即释放（[`Self::release_for_leave`]），
    /// 回首页按落盘进度续播（[`Self::resume_if_idle`]，Tauri 重挂载同语义）。
    pin_hold: bool,
    switching: bool,
    /// 系统播放器周期性激活/挂载用的窗口句柄（无 render 时也能把窗口提到前台）。
    window_handle: gpui_kit::AnyWindowHandle,
    /// 舞台矩形（GPUI 逻辑像素、左上原点）：系统播放器图层跟随它。
    stage_bounds: Rc<Cell<Option<[f32; 4]>>>,
    /// 系统播放器画面已就绪（首帧事件）：视图据此打透明洞并跳过 GPUI 上屏。
    system_video_ready: bool,
    /// 系统后端的窗口已经提到前台过一次（会话内一次）。
    ///
    /// 无人值守启动时进程可能不在前台，GPUI 就不请求帧，视频层也就没有第
    /// 一次 render 去挂载——首次起播必须把窗口提到前台。但**每次引擎重建
    /// 都提一次会在切集时抢焦点**（Windows 的 activate 还会合成一对 ALT 键
    /// 去骗前台策略），看别的应用时被拉回来、窗口最小化时被强制还原。
    system_window_activated: bool,
    /// 系统后端一次切集/起播的起点（首帧事件到达时结算耗时）。
    system_switch_started: Option<Instant>,
    /// 每一次起播/接力的逻辑代次：AVQueuePlayer 接力不换播放器，不能用
    /// 播放器实例区分「上一集/下一集」，片尾幂等靠这三元组。
    play_generation: u64,
    /// 当前这一代真正收到过 FirstFrame 的代次。
    first_frame_generation: u64,
    /// 已经因片尾推进过的代次（同一代只允许一次）。
    ended_generation: u64,
    /// 正在异步预注册/预加载的下一集 vid（防重复入队）。
    system_preload_pending: Option<String>,
    /// 舞台周期状态日志的上一跳（排障取证：switching/ready 状态）。
    stage_diag_at: Cell<Instant>,
    /// 切换目标（switching 时 overlay 显示它的封面与标题）。
    pending_title: Option<String>,
    /// 起播失败态（自动重试也失败后落这里；画面中央错误卡 + 重试——
    /// 失败必须可见，否则「切 tab / 换剧没反应」）。
    play_error: Option<String>,
    /// 冷启动自动开播按住（取证开关 HONGGUO_PAGE 等把启动目的地定向到
    /// 非首页时由壳置位）：信息流首批落定后 [`after_feed_loaded`]
    /// （home/feed.rs）不再起播——否则人还在别的页，首页的剧已经在播。
    /// 回首页走 [`Self::resume_if_idle`]，不经过这个闸。
    cold_start_held: bool,
    /// 自动重试记账位：只在引擎就位时复位（play_target 里清会死循环）。
    play_retried: bool,
    /// 舞台轻量提示（Tauri toast 同位，见 [`crate::toast`]）：弹幕拉取/
    /// 发送与互动登录指引的可见出口——此前这类反馈只有日志，用户看到的
    /// 就是「点了没反应」。
    toast: Option<crate::toast::Toast>,

    // —— 控制区（对齐 Tauri 版控制栏核心件） ——
    /// 倍速/音量/静音的现值（引擎侧同值；换集后重放给新引擎，跨启动走
    /// [`crate::playback_prefs`]）。
    rate: f64,
    volume: f32,
    muted: bool,
    /// 进度拖动中的目标秒（None = 跟随播放位置显示）。
    scrubbing: Option<f64>,
    /// 上一次真正下发给播放器的 scrub seek 时刻：拖动中的连续 seek 按它节流
    ///（Tauri 的 `<video>` 每次赋值 currentTime 浏览器会自行合并，播放器这侧
    /// 每发一次 seek 都要重冲流，必须限频；松手的那次不受限）。
    last_scrub_seek: Cell<Instant>,
    /// 连播锁定（Tauri bingeSeriesId 同款）：信息流里主动选了某集/点了画面
    /// = 要追这部，锁定后滚轮/↑↓ 切集而不是跟随换剧；Esc/点返回解除。
    binge: Option<String>,
    /// 选集浮层开着（贴控制栏向上弹，见 [`Self::render_picker`]）。
    picker_open: bool,
    /// 手动翻到的范围段（每段 30 集）：记设置时的当前集——当前集变了
    /// （连播/点选）覆盖作废，自动跟回当前集所在段（Tauri groupOverride）。
    picker_group_override: Option<(usize, u32)>,
    /// 选集入口按钮的矩形哨兵（与 progress_bounds 同模式）。
    ///
    /// 浮层「点外部关闭」要把**入口按钮本身**排除在外（Tauri 的锚点是
    /// 按钮 + 浮层）：点按钮那一下交给按钮的 on_click 收起；若捕获层先
    /// 把它关了，按钮的 on_click 又会把它打开——面板关不掉还反复横跳。
    picker_entry_bounds: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    /// 用户显式选择的清晰度（None = 自动/沿用）：跨集保留（Tauri 的
    /// definition 会话态同语义），传给后续每次起播的 prepare。
    preferred_definition: Option<u32>,
    /// 剧集简介（video_detail 的 series_intro，Tauri useSeriesDetailMeta
    /// 同源同语义）：按剧缓存（装饰性信息，失败静默降级）。
    intros: HashMap<String, String>,
    /// 简介拉取中的剧（去重）。
    intros_pending: HashSet<String>,
    /// 简介展开态（Tauri introExpanded：超过 40 字可展开/收起）。
    intro_expanded: bool,
    /// 弹幕总开关（Tauri danmakuOn，随偏好落盘）。
    danmaku_on: bool,
    /// 弹幕文字不透明度 0.1–1。
    dm_opacity: f32,
    /// 弹幕字号缩放 0.5–2。
    dm_font_scale: f32,
    /// 弹幕密度 0–1（入场按比例随机丢弃）。
    dm_density: f32,
    /// 弹幕显示区域 0.25–1（占舞台顶部高度比例）。
    dm_area: f32,
    /// 弹幕设置面板开着。
    dm_panel_open: bool,
    /// 当前集的弹幕（offset_ms 升序；每次进集重拉，Tauri useDanmaku 同款）。
    danmaku_items: Arc<Vec<Danmaku>>,
    /// 已拉取弹幕的集（vid）——同集不重拉，换集必重拉。
    danmaku_vid: String,
    danmaku_fetching: bool,
    /// 滚窗拉取状态（官方 7.3.9.32 协议：30 秒窗按播放位置推进，见
    /// `danmaku::DanmakuWindow`）。
    danmaku_window: danmaku::DanmakuWindow,
    /// 已入场游标（items 下标；seek 时二分重置）。
    danmaku_cursor: usize,
    /// 在飞的弹幕（泳道 + 入场墙钟）。
    danmaku_flights: Vec<DmFlight>,
    /// 每条泳道「最早可再入场」的媒体秒（laneFreeAt 同款）。
    danmaku_lane_free: Vec<f64>,
    /// 上一帧的播放位置（seek 跳变检测用）。
    danmaku_last_pos: f64,
    /// tick 的上次时刻（推进在飞弹幕的 dt 源）。
    danmaku_last_tick: Option<std::time::Instant>,
    /// 密度抽样的 LCG 状态。
    dm_rng: u64,
    /// 弹幕发送框输入（需要 window 创建，见 HomeView::new）。
    danmaku_input: Option<Entity<InputState>>,
    danmaku_sending: bool,
    /// 发送成功后清空输入框（set_value 需要 window，渲染期执行）。
    danmaku_clear_input: bool,
    /// 弹幕表情弹层开着（pill 里的笑脸钮开关；点外部收起）。
    danmaku_emoji_open: bool,
    /// 窄舞台的弹幕浮层输入开着（「弹」字笔钮开关；点外部收起）。
    danmaku_compact_open: bool,
    /// 弹幕设置滑杆（透明度/字号/密度/面积）。
    dm_opacity_slider: Entity<SliderState>,
    dm_font_slider: Entity<SliderState>,
    dm_density_slider: Entity<SliderState>,
    dm_area_slider: Entity<SliderState>,
    // —— 互动（点赞/收藏/评论，Tauri interaction-rail + comment-panel） ——
    /// mget 最近互动列表（当前集/剧命中才有「已赞/已追」红标）。
    interact_items: Arc<Vec<InteractionItem>>,
    interact_loaded: bool,
    /// 集级公开计数（剧集档案 detail 下发，匿名可见；起播后从档案读）。
    ep_comment_count: i64,
    ep_digg_count: i64,
    /// 全剧收藏数（video_detail followed_cnt，load_series_meta 顺带）。
    follow_count: i64,
    digg_pending: bool,
    collect_pending: bool,
    /// 当前剧连载状态（video_detail series_status：0=连载中 1=已完结；
    /// 互动栏追更键显隐判定源，meta 未到 = None）。
    series_status: Option<i64>,
    /// 当前剧追更态（跟随列表首页本地判成员；未登录/未到 = None）。
    follow_is_on: Option<bool>,
    follow_pending: bool,
    /// 每剧 (followed_cnt, series_status) 会话内缓存（load_intro 顺带）。
    series_meta_cache: HashMap<String, (i64, i64)>,
    /// 评论面板开着。
    comments_panel_open: bool,
    /// 当前集评论（已拍平去重；每次进集重拉——社交态不缓存复用）。
    comments: Arc<Vec<CommentItem>>,
    comments_total: i64,
    comments_cursor: String,
    comments_has_more: bool,
    comments_loading: bool,
    /// 已加载评论的集（vid）。
    comments_vid: String,
    /// 评论列表滚动位置（滚动条层与回跳共用同一句柄，跨帧稳定）。
    comments_scroll: gpui_kit::ScrollHandle,
    /// 表情选择器网格滚动位置（系统表情 / 远端表情各一，跨档不复位）。
    emoji_grid_scroll: gpui_kit::ScrollHandle,
    emoticon_grid_scroll: gpui_kit::ScrollHandle,
    /// 评论点赞的本地覆盖（乐观 ±1 与红心；key=comment_id）。
    comment_digg_override: HashMap<String, bool>,
    /// 展开回复的评论 id → 已拉回复（首页）与翻页游标。
    replies_open: HashSet<String>,
    replies: HashMap<String, Arc<Vec<ReplyItem>>>,
    replies_cursor: HashMap<String, String>,
    replies_loading: HashSet<String>,
    // —— 选集浮层「系列剧」tab ——
    /// 各季卡片（plan/v 相关作品·系列里季号可解析的条目；每剧一次）。
    related_seasons: HashMap<String, Arc<Vec<RelatedItem>>>,
    related_loading: HashSet<String>,
    /// 选集浮层当前 tab：true = 系列剧。
    picker_tab_series: bool,
    /// 已预约季（服务端离线预约列表 + 本地预约成功即插；未上线季按钮态）。
    reserved_ids: HashSet<String>,
    /// 预约请求在途（防重复提交）。
    reserve_pending: HashSet<String>,
    /// 离线预约列表是否已拉（会话级一次；预约按钮的已约态数据源）。
    reservations_loaded: bool,
    /// 回复目标：Some((comment_id, reply_to_reply_id, 被回复人昵称))。
    reply_to: Option<(String, Option<String>, String)>,
    /// 评论输入框（与弹幕发送框同款 InputState）。
    comment_input: Option<Entity<InputState>>,
    comment_sending: bool,
    /// 本地附件上传在飞（发表时才传，Tauri uploading 态同口径）。
    comment_uploading: bool,
    /// 本地图片附件草稿（选图不请求网络，发表时才上传）。
    comment_local_attachment: Option<comment_view::emoticon::LocalImageAttachment>,
    /// 表情面板搜索输入框。
    emoticon_search_input: Option<Entity<InputState>>,
    /// 在飞的表情图请求（按 `web_uri` 去重；渲染期每帧调
    /// request_emoticon_images，靠它保证滚动到哪只请求一次）。
    emoticon_fetching: Rc<std::cell::RefCell<HashSet<String>>>,
    comment_clear_input: bool,
    /// @提及候选（relation/list，含 AI bot「小果」）；按剧缓存一次。
    mention_users: Arc<Vec<hongguo_api::interact::MentionUser>>,
    mention_loaded_for: Option<String>,
    mention_pending: bool,
    /// 已插入待发送的提及对象 (uid, name)；发送成功即清。
    mention_user: Option<(String, String)>,
    /// 待写入输入框的提及段（候选点击 → render 期 set_value，形如
    /// `@名字\u{FFFC} `；set_value 要 window，不能就地写）。
    mention_insert: Option<String>,
    /// @小果 提及评论的 AI 回复轮询（Tauri aiPending 同款）：
    /// (评论 id, 起始时刻)；120s 未等到放弃。
    ai_pending: Vec<(String, Instant)>,
    ai_polling: bool,
    /// 评论面板的表情选择器开着（笑脸按钮 toggle；连选不关面板）。
    emoji_picker_open: bool,
    /// 表情面板状态（档位 + 第三方/自定义列表）。
    emoticon_panel: comment_view::emoticon::EmoticonPanel,
    /// 表情操作菜单目标（官方同款：长按表情 → 「添加到表情」/「删除表情」）。
    /// Some((表情, 是否可删)) = 菜单目标；None = 未打开。
    emoticon_menu: Option<(hongguo_api::emoticon::Emoticon, bool)>,
    /// 待确认删除的表情（删除走官方同款二次确认浮层）。
    emoticon_delete_target: Option<hongguo_api::emoticon::Emoticon>,
    /// 表情图解码缓存（URL → 位图；面板与评论里的图共用）。
    emoticon_images: std::collections::HashMap<String, Arc<gpui_kit::RenderImage>>,
    /// 待发送的评论附图（表情图片；官方两态：第三方 2 / 自定义 4）。
    comment_attachment: Option<(hongguo_api::emoticon::Emoticon, Option<i64>)>,
    // —— 壳内登录（SMS；Tauri 登录页对齐的最小面）——
    login_panel_open: bool,
    login_mobile: Option<Entity<InputState>>,
    login_code: Option<Entity<InputState>>,
    login_busy: bool,
    login_message: Option<String>,
    /// MFA 上行短信重试上下文（retry_tag, sms_code_key）。
    mfa_retry: Option<(String, String)>,
    /// 待确认删除目标：Some((id, 父评论 id——回复场景才有))，Tauri
    /// deleteTarget 同构；id 是 comment_id 或 reply_id（同走 comment/del
    /// service_id=4）。
    comment_delete_target: Option<(String, Option<String>)>,
    /// 删除请求在途（防重复提交）。
    comment_deleting: bool,
    // —— 下载 ——
    /// 下载面板（「下载到本地」选集浮层）开着。
    downloads_open: bool,
    /// 浮层里已勾选的集号（升序；默认 = 在播这一集）。
    download_selected: Vec<u32>,
    /// 区间语法输入框（Tauri `EpisodeRangePicker` 同款）。
    download_range: Entity<InputState>,
    /// 区间串解析不出任何集号（Tauri `rangeError` 同义）。
    download_range_error: bool,
    /// 隐身模式（Tauri incognito 同语义，见 [`incognito`]）：开着时光标
    /// 脱离窗口 → 暂停 + 整窗藏起，回到窗口矩形 → 原地重现 + 续播。
    incognito: bool,
    /// 隐身欠下的那次播放：藏窗瞬间本来在播才记欠账（Tauri owedPlay 同构），
    /// 回来补上；用户自己按停的不欠。
    incognito_owed: bool,
    /// 隐身轮询任务在跑（防重复起；关开关后由下一拍自杀收位）。
    incognito_polling: bool,
    /// 最小化欠下的那次播放（Tauri owedPlay 同构，见 [`minimize`]）：
    /// 最小化瞬间本来在播才记欠，还原补上。
    minimize_owed: bool,
    /// 全屏中（Render 时从 window.is_fullscreen 读，切图标用）。
    fullscreen: bool,
    /// 小屏播放态（Tauri miniScreen 同款）：Some 是进入前的几何快照。
    /// 同窗缩放语义——不换窗口、不暂停，只缩窗口 + 换紧凑控制条。
    mini: Option<crate::platform::WindowSnapshot>,
    /// 竖屏播放态（Tauri portrait 同款）：Some 是进入前的几何快照。
    /// 与小屏同一套机制（同窗收窄成竖条 + 紧凑控制条竖屏形态）。
    portrait: Option<crate::platform::WindowSnapshot>,
    /// 竖屏铺满档（Tauri portraitFull 同款；只在竖屏中有意义）。
    portrait_full: bool,
    /// 上次落窗用的竖屏比例（变化 >1% 才重适配，Tauri REFIT_DELTA 同值）。
    portrait_aspect_last: f64,
    /// 缺 HEVC 扩展的安装引导：本会话点过「暂不安装」就不再打扰（Tauri
    /// `hevcSession.markDismissed` 的模块级会话语义）。
    hevc_prompt_dismissed: bool,
    /// HEVC 扩展安装中（按钮转「下载安装中…」并禁用）。
    hevc_installing: bool,
    /// 沉浸全屏（播放器全屏按钮/F 键）：OS 全屏 + chrome（顶栏/侧栏）全藏。
    /// 顶栏最大化钮是另一档「应用全屏」——chrome 保留，对齐 Tauri。
    immersive: bool,
    /// 无边框播放（播放页 bare 布局同款，issue #11）：chrome 全藏
    /// 但**不进 OS 全屏**，窗口保持原几何；macOS 红绿灯一并藏
    /// （[`crate::macos`]）。与 [`Self::immersive`] 互斥。
    frameless: bool,
    /// 窗口钉住（Tauri usePinWindow 同款：窗口级会话态，大小屏共用）。
    pinned: bool,
    /// 下载任务快照（每次渲染从队列读，进度靠 service 事件与播放节拍驱动
    /// 的重渲染刷新）。
    downloads: Arc<Vec<DownloadTask>>,
    /// 选集浮层的下载完成集号（渲染时刷新）。
    done_vids: Arc<Vec<u32>>,
    /// 音量悬停弹层开着（Tauri VolumePopup 同款，随 wrapper hover 开合）。
    volume_popup_open: bool,
    /// 音量按钮 / 弹层本体各自的 hover 位：gpui 的 hover 按命中盒判定，
    /// 弹层在按钮盒外——两个 hover 取并集才是「弹层开着」。
    volume_hovered: bool,
    volume_popup_hovered: bool,
    /// 音量滑杆正被按住拖动：拖动期间弹层要钉住。
    ///
    /// gpui 进入拖拽捕获后，弹层的 hover 位会掉（指针被捕获，不再按命中
    /// 盒判定），hover 驱动的弹层当场收起 → 滑杆从元素树里消失 →
    /// drag_move 再也送不到它，音量停在按下处（用户实测「按住拖不动」）。
    /// 拖动中用这个位把弹层钉在屏幕上。
    volume_dragging: bool,
    /// 倍速 / 清晰度下拉菜单开着（Popover 的 on_open_change 回写）：开着期间
    /// 控制条常显——不进 `panel_open` 的话，重算时 3s 一到连菜单一起收掉，
    /// 菜单根本没法操作。
    rate_menu_open: bool,
    definition_menu_open: bool,
    /// 音量竖条的轨道矩形哨兵（拖动换算用，与 progress_bounds 同模式）。
    volume_bounds: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    /// 播放器缓冲状态（Buffering/Ready 事件驱动，中间出 loading）。
    buffering: bool,
    /// 控制条可见截止时刻；拖动中/暂停时常显。
    controls_until: Option<Instant>,
    /// 指针是否在舞台内：离开舞台 = 立即收悬浮层（Tauri hideChrome 同款），
    /// 在台内静止走 3s 倒计时（不因悬停画面而常显）。
    pointer_on_stage: bool,
    /// 指针悬在控制栏本体上：控件不倒计时（拖进度条/悬停音量时永不收，
    /// Tauri controlsHovered 同款；大小屏两套控制条都置位）。
    controls_hovered: bool,
    /// 指针悬在进度轨道上（滑块圆点显示开关，Tauri ScrubBar 同款）。
    progress_hovered: bool,
    /// 小窗拖窗的按下起点（画面层上判位移：<3px 当单击暂停，超过才起拖）。
    mini_drag: Option<gpui_kit::Point<gpui_kit::Pixels>>,
    /// 自动连播静默起步（Tauri quietStart 同款）：连播切集到新集首帧或
    /// 用户真实输入之间，一切悬浮层不亮（面板照常）。
    silent_until_input: bool,
    /// 进度轨道上帧矩形（拖动/点击的指针位置 → 比例换算用；同
    /// `stage_rects` 的哨兵存取模式）。
    progress_bounds: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,

    wheel_at: Option<Instant>,
    last_save: Instant,
    /// 观看历史上报节拍（`history_cadence`，与 Tauri 前端同一套规则）：
    /// 1 秒心跳按播放位置判定——新剧 5 秒首报、同剧切集立即翻页、
    /// 每 60 秒一拍；暂停/切集/退出走 force 收尾。
    history_cadence: HistoryCadence,
    /// 播放快捷键的焦点锚：键盘事件只派发给焦点子树，舞台持有一个
    /// FocusHandle（见 render_stage），首次渲染播种、点舞台带回。
    focus: gpui_kit::FocusHandle,
    focus_seeded: bool,
    /// 播放器快捷键映射（设置页可改；舞台的 on_key_down 读它）。
    shortcut_keys: crate::playback_prefs::ShortcutKeys,
    /// 已流预取（每部一次，失败不算数允许重试）。
    stream_prefetched: HashSet<String>,
    /// 已预取过分集档案的剧。
    archives_prefetched: HashSet<String>,
    /// 云端历史：推荐 tab 种子 + 起点决策兜底（启动拉一次）。
    history: Option<Vec<WatchHistoryItem>>,
    history_pending: bool,
    /// 「你的内容」种子（云端历史 + 书架）：独立于流缓存持有，换一批/翻页
    /// 都不动它（Tauri 的 seedItems memo 同款）。混进流数组会被后续
    /// `load_feed` 整体覆盖、或在头部插入后与流内同条目重影。
    seeds: Vec<StreamItem>,

    /// 舞台两层（封面占位层 / 视频槽）上次报上来的尺寸：图形路径没有视觉
    /// 测试，这行自检日志是唯一的哨兵——视频槽必须与封面层同为整块舞台，
    /// 不一致（或掉成 0×0）就是布局回归，不能只靠肉眼再发现一次。
    /// 存 `[封面w, 封面h, 视频w, 视频h]`。
    stage_rects: Rc<Cell<Option<[f32; 4]>>>,
}

impl gpui_kit::EventEmitter<HomeEvent> for HomeView {}

impl HomeView {
    /// 按住冷启动自动开播（见 [`Self::cold_start_held`] 字段注释）：
    /// 启动目的地不是首页时由壳调用。
    pub(crate) fn hold_cold_start(&mut self) {
        self.cold_start_held = true;
    }

    pub fn new(state: Arc<AppStateInner>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tabs = Source::fallback_tabs().to_vec();
        let mut feeds = HashMap::new();
        let mut indexes = HashMap::new();
        for s in &tabs {
            feeds.insert(s.clone(), SourceFeed::default());
            indexes.insert(s.clone(), 0usize);
        }
        // 偏好先于引擎：起播后重放都吃它
        let prefs = crate::playback_prefs::load_from(
            &hongguo_store::paths::data_dir().join(crate::playback_prefs::FILE_NAME),
        );
        let mut view = Self {
            state: Arc::clone(&state),
            active: Source::recommend(),
            feeds,
            indexes,
            tabs,
            tab_labels: HashMap::new(),
            stage_covers: HashMap::new(),
            epoch: Arc::new(AtomicU64::new(0)),
            player: None,
            #[cfg(feature = "screenshot")]
            preview_engine: false,
            now: None,
            pin_hold: true,
            switching: false,
            stage_diag_at: Cell::new(Instant::now()),
            pending_title: None,
            play_error: None,
            cold_start_held: false,
            play_retried: false,
            toast: None,
            rate: prefs.rate,
            volume: prefs.volume,
            muted: prefs.muted,
            scrubbing: None,
            last_scrub_seek: Cell::new(Instant::now() - Duration::from_secs(10)),
            binge: None,
            picker_open: false,
            picker_group_override: None,
            picker_entry_bounds: Rc::new(Cell::new(None)),
            preferred_definition: None,
            intros: HashMap::new(),
            intros_pending: HashSet::new(),
            intro_expanded: false,
            danmaku_on: prefs.danmaku_on,
            dm_opacity: prefs.dm_opacity,
            dm_font_scale: prefs.dm_font_scale,
            dm_density: prefs.dm_density,
            dm_area: prefs.dm_area,
            dm_panel_open: false,
            danmaku_items: Arc::new(Vec::new()),
            danmaku_vid: String::new(),
            danmaku_fetching: false,
            danmaku_window: danmaku::DanmakuWindow::default(),
            danmaku_cursor: 0,
            danmaku_flights: Vec::new(),
            danmaku_lane_free: Vec::new(),
            danmaku_last_pos: 0.0,
            danmaku_last_tick: None,
            dm_rng: 0x853c49e6748fea9b,
            danmaku_input: Some(cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(crate::i18n::t("player.interact.danmakuPlaceholder"))
            })),
            danmaku_sending: false,
            danmaku_clear_input: false,
            danmaku_emoji_open: false,
            danmaku_compact_open: false,
            dm_opacity_slider: cx.new(|_| {
                SliderState::new()
                    .min(0.1)
                    .max(1.0)
                    // 步长必须跟着区间走：SliderState 默认 step=1.0，0.1–1.0
                    // 的区间下只能落在 0.1 / 1.0 两档（用户实测「百分比要么
                    // 0 要么 100」）。
                    .step(0.05)
                    .default_value(prefs.dm_opacity)
            }),
            dm_font_slider: cx.new(|_| {
                SliderState::new()
                    .min(0.5)
                    .max(2.0)
                    .step(0.05)
                    .default_value(prefs.dm_font_scale)
            }),
            dm_density_slider: cx.new(|_| {
                SliderState::new()
                    .min(0.0)
                    .max(1.0)
                    .step(0.05)
                    .default_value(prefs.dm_density)
            }),
            dm_area_slider: cx.new(|_| {
                SliderState::new()
                    .min(0.25)
                    .max(1.0)
                    .step(0.05)
                    .default_value(prefs.dm_area)
            }),
            interact_items: Arc::new(Vec::new()),
            interact_loaded: false,
            ep_comment_count: 0,
            ep_digg_count: 0,
            follow_count: 0,
            digg_pending: false,
            collect_pending: false,
            series_status: None,
            follow_is_on: None,
            follow_pending: false,
            series_meta_cache: HashMap::new(),
            comments_panel_open: false,
            comments: Arc::new(Vec::new()),
            comments_total: 0,
            comments_cursor: String::new(),
            comments_has_more: false,
            comments_loading: false,
            comments_vid: String::new(),
            comments_scroll: gpui_kit::ScrollHandle::default(),
            emoji_grid_scroll: gpui_kit::ScrollHandle::default(),
            emoticon_grid_scroll: gpui_kit::ScrollHandle::default(),
            comment_digg_override: HashMap::new(),
            replies_open: HashSet::new(),
            replies: HashMap::new(),
            replies_cursor: HashMap::new(),
            replies_loading: HashSet::new(),
            related_seasons: HashMap::new(),
            related_loading: HashSet::new(),
            picker_tab_series: false,
            reserved_ids: HashSet::new(),
            reserve_pending: HashSet::new(),
            reservations_loaded: false,
            reply_to: None,
            comment_input: Some(cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(crate::i18n::t("player.comments.placeholder"))
            })),
            comment_sending: false,
            comment_uploading: false,
            comment_local_attachment: None,
            emoticon_search_input: Some(cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(crate::i18n::t("player.emoticon.searchPlaceholder"))
            })),
            emoticon_fetching: Rc::new(std::cell::RefCell::new(HashSet::new())),
            comment_clear_input: false,
            mention_users: Arc::new(Vec::new()),
            mention_loaded_for: None,
            mention_pending: false,
            mention_user: None,
            mention_insert: None,
            ai_pending: Vec::new(),
            ai_polling: false,
            emoji_picker_open: false,
            emoticon_panel: comment_view::emoticon::EmoticonPanel::default(),
            emoticon_menu: None,
            emoticon_delete_target: None,
            emoticon_images: HashMap::new(),
            comment_attachment: None,
            login_panel_open: false,
            // 字段标签在卡片里（Tauri FieldLabel 同款），输入框不再塞占位文案
            login_mobile: Some(cx.new(|cx| InputState::new(window, cx))),
            login_code: Some(cx.new(|cx| InputState::new(window, cx))),
            login_busy: false,
            login_message: None,
            mfa_retry: None,
            comment_delete_target: None,
            comment_deleting: false,
            downloads_open: false,
            download_selected: Vec::new(),
            download_range: cx.new(|cx| {
                InputState::new(window, cx).placeholder(crate::i18n::t("download.rangePlaceholder"))
            }),
            download_range_error: false,
            incognito: prefs.incognito,
            incognito_owed: false,
            incognito_polling: false,
            minimize_owed: false,
            fullscreen: false,
            mini: None,
            portrait: None,
            portrait_full: false,
            portrait_aspect_last: 0.0,
            hevc_prompt_dismissed: false,
            hevc_installing: false,
            immersive: false,
            frameless: false,
            pinned: false,
            downloads: Arc::new(Vec::new()),
            done_vids: Arc::new(Vec::new()),
            volume_popup_open: false,
            volume_hovered: false,
            volume_popup_hovered: false,
            volume_dragging: false,
            rate_menu_open: false,
            definition_menu_open: false,
            volume_bounds: Rc::new(Cell::new(None)),
            buffering: false,
            controls_until: None,
            pointer_on_stage: false,
            controls_hovered: false,
            progress_hovered: false,
            mini_drag: None,
            silent_until_input: false,
            progress_bounds: Rc::new(Cell::new(None)),
            wheel_at: None,
            last_save: Instant::now(),
            history_cadence: HistoryCadence::default(),
            focus: cx.focus_handle(),
            focus_seeded: false,
            shortcut_keys: prefs.keys.clone(),
            stream_prefetched: HashSet::new(),
            archives_prefetched: HashSet::new(),
            history: None,
            history_pending: false,
            seeds: Vec::new(),
            stage_rects: Rc::new(Cell::new(None)),
            window_handle: window.window_handle(),
            stage_bounds: Rc::new(Cell::new(None)),
            system_video_ready: false,
            system_window_activated: false,
            system_switch_started: None,
            play_generation: 0,
            first_frame_generation: 0,
            ended_generation: 0,
            system_preload_pending: None,
        };
        view.subscribe_controls(cx);
        view.subscribe_service_events(cx);
        // 表情面板搜索框：逐字前缀搜索（Tauri useQuery 按 keyword 重查同款；
        // 空关键词不发请求，见 search_emoticons 的门）
        if let Some(input) = view.emoticon_search_input.clone() {
            cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && this.emoticon_panel.tab == comment_view::emoticon::EmoticonTab::Search
                {
                    this.search_emoticons(cx);
                }
            })
            .detach();
        }
        // 隐身偏好跨启动还原（Tauri 挂载时重发 setIncognito(true) 同款）：
        // 窗口已就位就起轮询（两端都有藏显原语）
        if view.incognito {
            view.spawn_incognito_poller(window.window_handle(), cx);
        }
        // 最小化自动暂停监听（pauseOnMinimize，开关每拍现读）
        view.spawn_minimize_poller(window.window_handle(), cx);
        // 观看历史上报心跳（1s 一拍；没在播时内部直接返回）：节拍必须独立于
        // 5 秒落盘窗口，否则首报被量化、60 秒节拍根本不成立
        view.spawn_history_heartbeat(cx);
        // 下载队列恢复：running 视为中断改回 pending（Tauri bootstrap 同款），
        // 然后触发一轮调度把中断的任务续跑起来。库读取失败只记错误不拦
        // 启动——壳的其余功能不依赖队列（显式容错：有日志、只此一处）。
        {
            match view.state.store.tasks() {
                Ok(tasks) => {
                    let limit = view.state.settings().max_concurrency as usize;
                    view.state.replace_queue(
                        hongguo_service::download_service::queue::DownloadQueue::restore(
                            tasks, limit,
                        ),
                    );
                    hongguo_service::download_service::kick(view.state.clone());
                }
                Err(e) => log::error!("[Home] 下载队列恢复失败，本轮从空队列开始：{e}"),
            }
        }
        // 取证心跳（HONGGUO_DIAG=1 时启用）：2s 一跳。这行停了 = 主线程/
        // 前台执行器堵死（update 派发不到主线程），render 停跳的根因直接
        // 归位。平时不开，不污染日志。
        if std::env::var("HONGGUO_DIAG").is_ok() {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(2))
                        .await;
                    let ok = this
                        .update(cx, |_, cx| {
                            log::info!("[Home] 主线程心跳");
                            cx.notify();
                        })
                        .is_ok();
                    if !ok {
                        return;
                    }
                }
            })
            .detach();
        }
        // 预览进程只吃 fixtures 假快照，不发任何启动请求
        if !crate::preview::is_active() {
            view.load_feed(Source::recommend(), false, cx);
            view.load_history(cx);
            view.load_tabs(cx);
        }
        view
    }
}
