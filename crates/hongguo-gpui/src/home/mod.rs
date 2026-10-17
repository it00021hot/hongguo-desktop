//! 首页：沉浸式播放流（Tauri `home-page.tsx` 的 GPUI 对齐实现）。
//!
//! 视频铺满整页：滚轮在当前 tab 源内切上一部/下一部剧，剧名/集数/热度叠加
//! 在画面左下，切剧间隙封面占位。顶栏 tab 列表由服务端下发（bookmall/tab
//! 响应 tab_item[]，回落三件套推荐 16 / 漫剧 36 / 真人 39），同一接口供数，
//! **重复点当前 tab = 换一批**（重开会话、游标归零）；推荐 tab 把「你的
//! 内容」（云端历史 + 书架）种子排前，同一个号打开就是你的剧（对齐第三方
//! 与 Tauri 端）。
//!
//! 播放经 [`hongguo_playback`] 引擎：起点决策（本地档案 >3 秒续播，云端
//! 历史兜底）、5 秒节流落盘、首帧出画预取下一部的流、自动连播（末集接
//! 流内下一部——Tauri 的剧终接力跨季/换批语义后续对齐）。取流编排与
//! Tauri 同一段 service（运行时共用，见 `hongguo_service::runtime`）。
//!
//! 控制区（对齐 Tauri 版控制栏的核心件）：暂停/继续（按钮 + 点击画面）、
//! 进度条拖动 seek、倍速菜单（0.75–3x，音频变速不变调）、音量/静音；偏好
//! 经 [`crate::playback_prefs`] 跨集跨启动保留。键盘快捷键（空格/方向键）
//! 与 ↑↓ 切剧随控制层下一批目标落地；「回首页 5 分钟换一批」的过期重开
//! 已对齐（FEED_STALE 判定 + pin_hold 在播不动）。
//!
//! 本文件只留视图本体（HomeView/NowPlaying 状态、构造、订阅、引擎事件
//! 循环与 Render 入口）；按域拆出的子模块：`feed`（取流/展示列表）、
//! `playback`（播放编排）、`danmaku`（弹幕）、`comments`（评论/回复）、
//! `interact`（点赞/收藏/互动栏）、`auth`（登录/头像）、`controls`（控制条
//! 与进度轨道）、`mini_panel`（小屏）、`picker`(选集浮层)、`downloads`
//! （下载面板）、`stage`（舞台/叠加层渲染）、`fmt`（格式化）。

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Entity, RenderImage, Window, div};
use hongguo_api::danmaku::{CommentItem, Danmaku, ReplyItem};
use hongguo_api::detail::RelatedItem;
use hongguo_api::history::WatchHistoryItem;
use hongguo_api::interact::InteractionItem;
use hongguo_common::model::{DownloadTask, VideoDefinition};
use hongguo_media_platform_api::video::VideoFrame;
use hongguo_playback::{PlayerEvent, PlayerHandle};
use hongguo_service::app_state::AppStateInner;
use hongguo_service::events::ServiceEvent;
use tokio::sync::mpsc;

use danmaku::DmFlight;
pub(crate) use feed::Source;
use feed::{SourceFeed, StreamItem};

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
}

/// 视频转换目标尺寸的跨线程打包（(w,h) ⇄ u64；Arc<AtomicU64> 共享给
/// 后台转换线程，见 [`HomeView::video_target`]）。
#[cfg(target_os = "windows")]
fn pack_size(w: usize, h: usize) -> u64 {
    ((w as u64) << 32) | h as u64
}

#[cfg(target_os = "windows")]
fn unpack_size(packed: u64) -> (usize, usize) {
    ((packed >> 32) as usize, (packed & 0xffff_ffff) as usize)
}

pub(crate) mod auth;
pub(crate) mod comment_view;
pub(crate) mod comments;
pub(crate) mod controls;
pub(crate) mod danmaku;
pub(crate) mod downloads;
pub(crate) mod downloads_range;
pub(crate) mod feed;
pub(crate) mod fmt;
pub(crate) mod hevc_prompt;
pub(crate) mod incognito;
pub(crate) mod interact;
pub(crate) mod mini_panel;
pub(crate) mod minimize;
pub(crate) mod overlay;
pub(crate) mod picker;
pub(crate) mod playback;
pub(crate) mod portrait_panel;
#[cfg(feature = "screenshot")]
pub(crate) mod preview_seed;
pub(crate) mod season;
pub(crate) mod stage;

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

    /// 已解码封面（series_id → RGBA）：切剧间隙的全屏占位。
    covers: HashMap<String, Arc<RenderImage>>,
    /// 已解码评论/回复头像（avatar URL → RGBA）；加载失败不入表（退回首字
    /// 圆形占位）。
    avatars: HashMap<String, Arc<RenderImage>>,
    avatars_pending: HashSet<String>,
    covers_pending: HashSet<String>,

    // —— 播放态 ——
    /// 引擎出生纪元：每次切剧 +1，陈旧事件/任务按它丢弃。Arc 原子共享给
    /// 事件循环——转换挪到后台线程后，纪元判定必须在**转换之前**做
    ///（切剧窗口里旧帧不再白转一遍），主线程字段够不着就要共享。
    epoch: Arc<AtomicU64>,
    player: Option<std::sync::Arc<PlayerHandle>>,
    /// 预览的虚拟引擎标记（screenshot feature）：预览无真实引擎，而控制条/
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
    /// 当前可上屏的帧；`None` = 还没出画（画封面占位）。
    ///
    /// 出画与否**只认这一处**：曾经另立过 `frame_ready` 标志，和「引擎就位」
    /// 更新互相抢时序——首帧先到置 true、就位后到又清 false，封面从此永远
    /// 收不掉（画面上封面与视频叠着）。帧直接来自事件，没有第二处状态。
    frame: Option<VideoFrame>,
    /// Windows 的渲染产物：NV12 → RGBA（BT.709 limited）在事件任务里转换
    /// （不占 UI 线程），渲染按普通图像上传——gpui 的视频 surface 只有
    /// Apple 目标（CVPixelBuffer），Windows 走这条等价路径。
    #[cfg(target_os = "windows")]
    frame_image: Option<Arc<RenderImage>>,
    /// 已下台、等 render 里 `window.drop_image` 回收的图像。gpui 的纹理按
    /// `RenderImage::id` 常驻 sprite atlas（`paint_image` 只增不删），视频
    /// 路径每帧一个新 id，不主动回收就是无上限的显存/内存泄漏。
    #[cfg(target_os = "windows")]
    retired_images: Vec<Arc<RenderImage>>,
    /// 视频转换目标尺寸（宽,高；CSS px，打包成 u64 跨线程共享——转换在
    /// gpui 后台线程池跑，Rc/Cell 过不去）。render_stage 按舞台显示区刷新，
    /// 事件循环的 NV12→RGBA 按它缩放输出——769px 的视口出 1080p 纹理纯
    /// 浪费：转换 CPU、图集纹理、回收压力都按面积走（8.3MB → ~1.3MB）。
    #[cfg(target_os = "windows")]
    video_target: Arc<AtomicU64>,
    /// 转换产物是否按「铺满裁切」（cover）适配：小屏形态为真——Tauri 小屏
    /// 把视频 `object-cover` 拉满 480×270（竖屏剧在小窗里也不留黑边），
    /// 其余形态 object-contain 留黑边。转换线程读它，舞台渲染写它。
    #[cfg(target_os = "windows")]
    video_cover: Arc<std::sync::atomic::AtomicBool>,
    /// 舞台周期状态日志的上一跳（排障取证：frame/switching/image 状态）。
    stage_diag_at: Cell<Instant>,
    /// 切换目标（switching 时 overlay 显示它的封面与标题）。
    pending_title: Option<String>,
    /// 起播失败态（自动重试也失败后落这里；画面中央错误卡 + 重试——
    /// 失败必须可见，否则「切 tab / 换剧没反应」）。
    play_error: Option<String>,
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
    /// 上一次真正下发给引擎的 scrub seek 时刻：拖动中的连续 seek 按它节流
    ///（Tauri 的 `<video>` 每次赋值 currentTime 浏览器会自行合并，引擎这侧
    /// 每发一次就是冲流 + 解码器重建，必须限频；松手的那次不受限）。
    last_scrub_seek: Cell<Instant>,
    /// 连播锁定（Tauri bingeSeriesId 同款）：信息流里主动选了某集/点了画面
    /// = 要追这部，锁定后滚轮/↑↓ 切集而不是跟随换剧；Esc/点返回解除。
    binge: Option<String>,
    /// 选集浮层开着（贴控制栏向上弹，见 [`Self::render_picker`]）。
    picker_open: bool,
    /// 手动翻到的范围段（每段 30 集）：记设置时的当前集——当前集变了
    /// （连播/点选）覆盖作废，自动跟回当前集所在段（Tauri groupOverride）。
    picker_group_override: Option<(usize, u32)>,
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
    /// 已入场游标（items 下标；seek 时二分重置）。
    danmaku_cursor: usize,
    /// 在飞的弹幕（泳道 + 入场墙钟）。
    danmaku_flights: Vec<DmFlight>,
    /// 每条泳道「最早可再入场」的媒体秒（hgplayer laneFreeAt 同款）。
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
    #[cfg(target_os = "windows")]
    incognito_polling: bool,
    /// 最小化欠下的那次播放（Tauri owedPlay 同构，见 [`minimize`]）：
    /// 最小化瞬间本来在播才记欠，还原补上。
    #[cfg(target_os = "windows")]
    minimize_owed: bool,
    /// 全屏中（Render 时从 window.is_fullscreen 读，切图标用）。
    fullscreen: bool,
    /// 小屏播放态（Tauri miniScreen 同款）：Some 是进入前的几何快照。
    /// 同窗缩放语义——不换窗口、不暂停，只缩窗口 + 换紧凑控制条。
    mini: Option<crate::mini::SavedGeometry>,
    /// 竖屏播放态（Tauri portrait 同款）：Some 是进入前的几何快照。
    /// 与小屏同一套机制（同窗收窄成竖条 + 紧凑控制条竖屏形态）。
    portrait: Option<crate::mini::SavedGeometry>,
    /// 竖屏铺满档（Tauri portraitFull 同款；只在竖屏中有意义）。
    portrait_full: bool,
    /// 上次落窗用的竖屏比例（变化 >1% 才重适配，Tauri REFIT_DELTA 同值）。
    portrait_aspect_last: f64,
    /// 同剧切集保留上一集末帧顶住画面（Tauri `use-frame-hold` 同款：切集
    /// 全程不闪封面，直到新集首帧出画；切剧仍立刻走封面）。
    hold_last_frame: bool,
    /// 缺 HEVC 扩展的安装引导：本会话点过「暂不安装」就不再打扰（Tauri
    /// `hevcSession.markDismissed` 的模块级会话语义）。
    hevc_prompt_dismissed: bool,
    /// HEVC 扩展安装中（按钮转「下载安装中…」并禁用）。
    hevc_installing: bool,
    /// 沉浸全屏（播放器全屏按钮/F 键）：OS 全屏 + chrome（顶栏/侧栏）全藏。
    /// 顶栏最大化钮是另一档「应用全屏」——chrome 保留，对齐 Tauri。
    immersive: bool,
    /// 窗口钉住（Tauri usePinWindow 同款：窗口级会话态，大小屏共用）。
    pinned: bool,
    /// 下载任务快照（每次渲染从队列读，进度靠 Frame 事件驱动的重渲染刷新）。
    downloads: Arc<Vec<DownloadTask>>,
    /// 选集浮层的下载完成集号（渲染时刷新）。
    done_vids: Arc<Vec<u32>>,
    /// 音量悬停弹层开着（Tauri VolumePopup 同款，随 wrapper hover 开合）。
    volume_popup_open: bool,
    /// 音量按钮 / 弹层本体各自的 hover 位：gpui 的 hover 按命中盒判定，
    /// 弹层在按钮盒外——两个 hover 取并集才是「弹层开着」。
    volume_hovered: bool,
    volume_popup_hovered: bool,
    /// 倍速 / 清晰度下拉菜单开着（Popover 的 on_open_change 回写）：开着期间
    /// 控制条常显——不进 `panel_open` 的话，帧事件流触发重算时 3s 一到连
    /// 菜单一起收掉，菜单根本没法操作（暂停才能点得到的根源）。
    rate_menu_open: bool,
    definition_menu_open: bool,
    /// 音量竖条的轨道矩形哨兵（拖动换算用，与 progress_bounds 同模式）。
    volume_bounds: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    /// 引擎 rebuffer 状态（Buffering/Ready 事件驱动，中间出 loading）。
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
    /// 自动连播静默起步（Tauri quietStart 同款）：连播切集到新集首帧或
    /// 用户真实输入之间，一切悬浮层不亮（面板照常）。
    silent_until_input: bool,
    /// 进度轨道上帧矩形（拖动/点击的指针位置 → 比例换算用；同
    /// `stage_rects` 的哨兵存取模式）。
    progress_bounds: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,

    wheel_at: Option<Instant>,
    last_save: Instant,
    /// 观看历史记录节拍（issue #10 节奏对齐 Tauri）：键 = series:集号，
    /// 切集清零；本集已记录次数（起播 ≥5 秒首报一次，之后由 force 事件
    /// ——暂停/切集/离开——补记，与 Tauri 实际行为一致）。
    history_report_key: String,
    history_reported: u64,
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
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            avatars: HashMap::new(),
            avatars_pending: HashSet::new(),
            epoch: Arc::new(AtomicU64::new(0)),
            player: None,
            #[cfg(feature = "screenshot")]
            preview_engine: false,
            now: None,
            pin_hold: true,
            switching: false,
            frame: None,
            #[cfg(target_os = "windows")]
            frame_image: None,
            #[cfg(target_os = "windows")]
            retired_images: Vec::new(),
            #[cfg(target_os = "windows")]
            video_target: Arc::new(AtomicU64::new(pack_size(1280, 720))),
            #[cfg(target_os = "windows")]
            video_cover: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            stage_diag_at: Cell::new(Instant::now()),
            pending_title: None,
            play_error: None,
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
            danmaku_cursor: 0,
            danmaku_flights: Vec::new(),
            danmaku_lane_free: Vec::new(),
            danmaku_last_pos: 0.0,
            danmaku_last_tick: None,
            dm_rng: 0x853c49e6748fea9b,
            danmaku_input: Some(cx.new(|cx| InputState::new(window, cx).placeholder("发条弹幕…"))),
            danmaku_sending: false,
            danmaku_clear_input: false,
            danmaku_emoji_open: false,
            dm_opacity_slider: cx.new(|_| {
                SliderState::new()
                    .min(0.1)
                    .max(1.0)
                    .default_value(prefs.dm_opacity)
            }),
            dm_font_slider: cx.new(|_| {
                SliderState::new()
                    .min(0.5)
                    .max(2.0)
                    .default_value(prefs.dm_font_scale)
            }),
            dm_density_slider: cx.new(|_| {
                SliderState::new()
                    .min(0.0)
                    .max(1.0)
                    .default_value(prefs.dm_density)
            }),
            dm_area_slider: cx.new(|_| {
                SliderState::new()
                    .min(0.25)
                    .max(1.0)
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
            comment_input: Some(cx.new(|cx| InputState::new(window, cx).placeholder("说点什么…"))),
            comment_sending: false,
            comment_clear_input: false,
            mention_users: Arc::new(Vec::new()),
            mention_loaded_for: None,
            mention_pending: false,
            mention_user: None,
            mention_insert: None,
            ai_pending: Vec::new(),
            ai_polling: false,
            emoji_picker_open: false,
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
            download_range: cx
                .new(|cx| InputState::new(window, cx).placeholder("如 1-50 或 1-10, 25, 30-45")),
            download_range_error: false,
            incognito: prefs.incognito,
            incognito_owed: false,
            #[cfg(target_os = "windows")]
            incognito_polling: false,
            #[cfg(target_os = "windows")]
            minimize_owed: false,
            fullscreen: false,
            mini: None,
            portrait: None,
            portrait_full: false,
            portrait_aspect_last: 0.0,
            hold_last_frame: false,
            hevc_prompt_dismissed: false,
            hevc_installing: false,
            immersive: false,
            pinned: false,
            downloads: Arc::new(Vec::new()),
            done_vids: Arc::new(Vec::new()),
            volume_popup_open: false,
            volume_hovered: false,
            volume_popup_hovered: false,
            rate_menu_open: false,
            definition_menu_open: false,
            volume_bounds: Rc::new(Cell::new(None)),
            buffering: false,
            controls_until: None,
            pointer_on_stage: false,
            controls_hovered: false,
            progress_hovered: false,
            silent_until_input: false,
            progress_bounds: Rc::new(Cell::new(None)),
            wheel_at: None,
            last_save: Instant::now(),
            history_report_key: String::new(),
            history_reported: 0,
            focus: cx.focus_handle(),
            focus_seeded: false,
            shortcut_keys: prefs.keys.clone(),
            stream_prefetched: HashSet::new(),
            archives_prefetched: HashSet::new(),
            history: None,
            history_pending: false,
            seeds: Vec::new(),
            stage_rects: Rc::new(Cell::new(None)),
        };
        view.subscribe_controls(cx);
        view.subscribe_service_events(cx);
        // 隐身偏好跨启动还原（Tauri 挂载时重发 setIncognito(true) 同款）：
        // 有原生句柄才起轮询，预览/无头环境保持关
        #[cfg(target_os = "windows")]
        if view.incognito
            && let Some(hwnd) = crate::win32::hwnd_of(window)
        {
            view.spawn_incognito_poller(hwnd, cx);
        }
        #[cfg(not(target_os = "windows"))]
        if view.incognito {
            view.incognito = false;
        }
        // 最小化自动暂停监听（pauseOnMinimize，开关每拍现读）：同样要原生
        // 句柄，预览/无头环境与 macOS 不起
        #[cfg(target_os = "windows")]
        if let Some(hwnd) = crate::win32::hwnd_of(window) {
            view.spawn_minimize_poller(hwnd, cx);
        }
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

    /// tab 的显示名：服务端下发标题优先，缺省回落本地名。
    pub(crate) fn tab_label(&self, s: &Source) -> String {
        self.tab_labels
            .get(s.as_str())
            .filter(|t| !t.is_empty())
            .cloned()
            .unwrap_or_else(|| s.fallback_label().to_string())
    }

    /// 顶栏中部 tab 槽（Tauri `TopBarTabs` portal 同位）：壳在顶栏渲染它，
    /// 点 tab 换源。列表由服务端下发（回落三件套见 `feed::Source`）。
    /// 舞台里的悬浮 tab 行已随顶栏化退场（Tauri 只有顶栏一处）。
    pub(crate) fn render_top_tabs(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        if self.tabs.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .id("top-feed-tabs")
                .gap_1()
                .children(self.tabs.iter().cloned().map(|s| {
                    let active = s == self.active;
                    let button = Button::new(format!("stage-tab-{}", s.as_str()))
                        .label(self.tab_label(&s))
                        .small();
                    let button = if active {
                        button.secondary()
                    } else {
                        button.ghost()
                    };
                    button.on_click(cx.listener(move |this, _, _, cx| {
                        this.switch_source(s.clone(), cx);
                        cx.stop_propagation();
                    }))
                }))
                .into_any_element(),
        )
    }

    /// 窗口是否已置顶（顶栏钮的图标态；与小屏紧凑条共用 `pinned`）。
    pub(crate) fn pinned(&self) -> bool {
        self.pinned
    }

    /// 设置页改了快捷键映射：换进运行中的视图（新值已由设置页落盘）。
    pub(crate) fn reload_shortcut_keys(
        &mut self,
        keys: crate::playback_prefs::ShortcutKeys,
        cx: &mut Context<Self>,
    ) {
        self.shortcut_keys = keys;
        cx.notify();
    }

    /// 弹一条轻量提示（Tauri toast 同位）：单槽覆盖，定时收尾（不依赖
    /// 重渲染节拍——暂停中没有 Frame 事件也能按时消失）。
    pub(crate) fn show_toast(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.toast = Some(crate::toast::Toast::new(message));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::toast::TOAST_TTL)
                .await;
            let _ = this.update(cx, |view, cx| {
                if view
                    .toast
                    .as_ref()
                    .is_some_and(crate::toast::Toast::expired)
                {
                    view.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 订阅 service 业务事件（登录 MFA、下载状态）并落到视图状态。
    ///
    /// 事件泵把 tokio broadcast 转成 GPUI 侧可 await 的无界通道
    /// （见 [`crate::events`]）；这里起一条消费循环——与 Tauri 壳消费
    /// 同一批事件（一份业务实现，两种 UI 适配）。
    fn subscribe_service_events(&mut self, cx: &mut Context<Self>) {
        let mut rx = crate::events::subscribe(&self.state);
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                let ok = this
                    .update(cx, |view, cx| view.on_service_event(event, cx))
                    .is_ok();
                if !ok {
                    break; // 视图已销毁
                }
            }
        })
        .detach();
    }

    /// service 事件 → 视图状态。
    fn on_service_event(&mut self, event: ServiceEvent, cx: &mut Context<Self>) {
        match event {
            ServiceEvent::LoginMfa(ev) => self.on_login_mfa_event(ev, cx),
            // 下载面板开着时随事件即时刷新（面板本身另有 1s 轮询兜底）
            ServiceEvent::DownloadTaskAdded(_)
            | ServiceEvent::DownloadProgress(_)
            | ServiceEvent::DownloadCompleted(_)
            | ServiceEvent::DownloadFailed(_)
            | ServiceEvent::DownloadStopped(_)
            | ServiceEvent::DownloadQueueChanged(_) => {
                if self.downloads_open {
                    self.refresh_downloads(cx);
                }
            }
            ServiceEvent::OnlineProgress(_) => {}
            ServiceEvent::MergeTaskAdded(_)
            | ServiceEvent::MergeProgress(_)
            | ServiceEvent::MergeCompleted(_)
            | ServiceEvent::MergeFailed(_) => {}
            ServiceEvent::CompatProgress(_) => {}
            // 追更提醒：GPUI 壳的追更页未建，先记日志（通知已落本地库，
            // 页面就位后从这里接 UI 刷新）
            ServiceEvent::FollowNotices(ev) => {
                log::info!("[Follow] {} 条新更新提醒", ev.notices.len());
            }
        }
    }

    /// 引擎是否就位：控制条、选集/下载面板渲染的共同门槛。真实壳=引擎
    /// 句柄在；预览进程由种子置位虚拟引擎（screenshot feature）。
    fn engine_ready(&self) -> bool {
        self.player.is_some() || self.preview_engine()
    }

    #[cfg(feature = "screenshot")]
    fn preview_engine(&self) -> bool {
        self.preview_engine
    }

    #[cfg(not(feature = "screenshot"))]
    fn preview_engine(&self) -> bool {
        false
    }

    // —— 壳协作面（Shell 顶栏/侧栏读写的最小开口） ——

    /// 当前 tab 源（预览交互探针读，断言切源生效）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn active_source(&self) -> Source {
        self.active.clone()
    }

    /// 切沉浸流 tab（顶栏 tab 入口）：重复点当前 tab = 换一批（重开会话、
    /// 游标归零、在播剧钉住不动）；切到别的 tab 关掉在播钉——新源首批到位
    /// 或缓存游标处的剧立即接管播放，切 tab 就换片（Tauri pinHold 同口径）。
    pub(crate) fn switch_source(&mut self, s: Source, cx: &mut Context<Self>) {
        log::info!(
            "[Home] 切换 tab 源 → {}（当前 {}）",
            self.tab_label(&s),
            self.tab_label(&self.active)
        );
        let same = self.active == s;
        let empty = self.feeds.get(&s).is_none_or(|f| f.items.is_empty());
        if same {
            // 换一批：重开会话、游标归零，在播剧钉住不动（开闸）
            self.pin_hold = true;
            self.load_feed(s, true, cx);
            return;
        }
        // 跨 tab 关闸：钉子不关的话，游标 0 会被在播剧占住，首批到位后
        // follow_cursor/play_target 判「目标就是在播」而不切——点了没反应
        self.pin_hold = false;
        self.active = s.clone();
        if empty {
            self.load_feed(s, false, cx);
        } else {
            let cursor_item = self
                .display_items()
                .get(self.cursor())
                .map(|i| i.series_id.clone());
            let target_same = self
                .now
                .as_ref()
                .is_some_and(|n| cursor_item.as_deref() == Some(&n.series_id));
            if !target_same && let Some(item) = self.display_items().get(self.cursor()).cloned() {
                self.play_target(item, None, None, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn immersive_fullscreen(&self) -> bool {
        self.immersive
    }

    /// 置顶窗口（顶栏入口与小屏紧凑条共用；FFI 成功才翻状态，两边不各说
    /// 各话——Tauri usePinWindow 同一份语义）。
    pub(crate) fn toggle_pinned(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = !self.pinned;
        if crate::mini::set_topmost(window, cx, next) {
            self.pinned = next;
            log::info!("[Home] 窗口钉住 → {next}");
            cx.notify();
        }
    }

    /// 播放器全屏（控制条按钮 / F / Esc）：OS 全屏 + chrome（顶栏/侧栏）
    /// 全藏 = 真沉浸。与「应用全屏」（顶栏最大化钮，chrome 保留）分层。
    pub(super) fn toggle_immersive_fullscreen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.immersive = !self.immersive;
        window.toggle_fullscreen();
        self.wake_controls(cx);
        cx.notify();
    }

    fn subscribe_controls(&mut self, cx: &mut Context<Self>) {
        // 弹幕设置滑杆：Change 即生效，Release 落盘（音量滑杆同节奏）
        let dm_sliders = [
            (&self.dm_opacity_slider, 0u8),
            (&self.dm_font_slider, 1),
            (&self.dm_density_slider, 2),
            (&self.dm_area_slider, 3),
        ];
        for (slider, which) in dm_sliders {
            cx.subscribe(
                slider,
                move |this: &mut Self, _: Entity<SliderState>, event: &SliderEvent, cx| {
                    this.on_dm_slider_event(which, event, cx);
                },
            )
            .detach();
        }
        // 弹幕发送框：Enter 提交
        if let Some(input) = &self.danmaku_input {
            cx.subscribe(
                input,
                |this: &mut Self, _: Entity<InputState>, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit_danmaku(cx);
                    }
                },
            )
            .detach();
        }
        // 评论输入框：Enter 提交
        if let Some(input) = &self.comment_input {
            cx.subscribe(
                input,
                |this: &mut Self, _: Entity<InputState>, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit_comment(cx);
                    }
                },
            )
            .detach();
        }
        // 下载浮层的区间语法：输入即解析（Tauri `applyRange` 同款——
        // 部分能解析就采用能解析的部分，全部解析不出才标红）
        cx.subscribe(
            &self.download_range,
            |this: &mut Self, input: Entity<InputState>, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = input.read(cx).value().to_string();
                this.apply_download_range(&text, cx);
            },
        )
        .detach();
    }

    /// 引擎事件循环（每个引擎一个，epoch 封口：换剧后陈旧事件自然断流）。
    /// 纪元与转换目标经 Arc 原子共享——NV12→RGBA 转换挪到 gpui 后台线程池
    /// （每帧 3-4ms 标量转换 + BGRA 交换不再挤占主线程，30fps ≈ 每秒
    /// 120-150ms 主线程时间还给它），纪元判定因此必须走在转换**之前**。
    pub(super) fn spawn_event_loop(
        &mut self,
        epoch: u64,
        epoch_shared: Arc<AtomicU64>,
        mut events: mpsc::Receiver<PlayerEvent>,
        #[cfg(target_os = "windows")] video_target: Arc<AtomicU64>,
        #[cfg(target_os = "windows")] video_cover: Arc<std::sync::atomic::AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some(first) = events.recv().await {
                // 纪元先行判定：切剧/离开首页后本循环的剩余事件全部作废，
                // 直接退场——以前是整帧转换完才在 update 里丢弃，切剧窗口
                // （恰好是 UI 最忙的时候）负载凭空翻倍
                if epoch_shared.load(Ordering::Relaxed) != epoch {
                    return;
                }
                // 合并积压：Frame 只保最新一批里的最后一帧（1080p 转换 ~50ms、
                // 上屏只认最新，旧帧转换纯属浪费），其余事件一个不丢、按序处理。
                // 抽干按 16 条封顶：病态积压下（转换跟不上、通道满载）一条
                // Frame 事件背一帧 NV12（~3MB），不封顶就是单轮上百 MB 的
                // batch 分配；多余的下一轮再收，通道背压照旧作用在解码端。
                let mut batch = Vec::with_capacity(4);
                batch.push(first);
                while batch.len() < 16
                    && let Ok(next) = events.try_recv()
                {
                    batch.push(next);
                }
                let last_frame = batch.iter().rposition(|e| matches!(e, PlayerEvent::Frame(_)));
                'batch: for (i, event) in batch.into_iter().enumerate() {
                    if matches!(event, PlayerEvent::Frame(_)) && Some(i) != last_frame {
                        continue;
                    }
                    // Windows：NV12→RGBA 在后台线程池转换（不占主线程； awaiting
                    // 期间主线程照常跑布局/绘制/输入）；macOS 是 CVPixelBuffer
                    // 零拷贝，无需转换
                    #[cfg(target_os = "windows")]
                    let converted = match &event {
                        PlayerEvent::Frame(frame) => {
                            let target = unpack_size(video_target.load(Ordering::Relaxed));
                            let cover = video_cover.load(Ordering::Relaxed);
                            let frame = frame.clone();
                            cx.background_executor()
                                .spawn(async move {
                                    Self::render_image_of(&frame, target, cover)
                                })
                                .await
                        }
                        _ => None,
                    };
                    let ok = this
                        .update(cx, |view, cx| {
                            if view.epoch.load(Ordering::Relaxed) != epoch {
                                log::warn!(
                                    "[Home] 事件被纪元弃置（loop={epoch}，view={}）——事件到得了 view 却进不去",
                                    view.epoch.load(Ordering::Relaxed)
                                );
                                return;
                            }
                            #[cfg(target_os = "windows")]
                            if let Some(image) = converted {
                                view.set_frame_image(image);
                            }
                            view.on_player_event(event, cx);
                        })
                        .is_ok();
                    if !ok {
                        break 'batch;
                    }
                }
            }
        })
        .detach();
    }
}

impl gpui_kit::Render for HomeView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // 取证：render 调用率（64 次一跳）。绘制饿死时这行不涨而事件仍在流。
        static RENDERS: AtomicU32 = AtomicU32::new(0);
        let n = RENDERS.fetch_add(1, Ordering::Relaxed);
        if n.is_multiple_of(64) {
            log::info!("[Home] render #{n}");
        }
        // 纹理回收口：gpui 的 sprite atlas 按 RenderImage::id 常驻（paint 只增
        // 不删），视频每帧一个新 id——不在这里显式 drop，显存/内存就无上限
        // 涨（实测 5 分钟 5.5GB）。只在 render 时有 Window 句柄可调。
        #[cfg(target_os = "windows")]
        for old in std::mem::take(&mut self.retired_images) {
            if let Err(e) = window.drop_image(old) {
                log::warn!("[Home] 纹理回收失败: {e}");
            }
        }
        // 竖屏重适配：换集/换清晰度后档位尺寸变了就按新比例重落窗（比例
        // 变化 ≤1% 不动）。挂在这里是因为它是「起播完成后」唯一一定有
        // Window 的钩子——起播的异步收尾处拿不到 Window。
        self.refit_portrait(window, cx);
        // render_* 要 &mut cx，先取（theme 借的是 cx 的不可变引用）。
        // 分类 tab 已移入壳顶栏（Shell render_title_bar），舞台从 tab 行
        // 下方起排改为吃满内容区（Tauri 沉浸流同款）。
        // 播放快捷键的焦点种子：键盘事件只派发给焦点子树，舞台的
        // FocusHandle 得在首次渲染时拿到焦点（只播种一次——之后焦点可能
        // 在弹层菜单里，每帧强抢会打架；点舞台会带回焦点）。
        // 放在 theme 借用之前：focus 要可变借 cx。
        if !self.focus_seeded && !self.focus.is_focused(window) {
            self.focus_seeded = true;
            window.focus(&self.focus, cx);
        }
        // 弹幕发送成功后的输入框清空（set_value 要 window，这里补执行）
        if self.danmaku_clear_input
            && let Some(input) = &self.danmaku_input
        {
            self.danmaku_clear_input = false;
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        // 评论输入框的延迟写入（set_value 要 window）：
        // 1) 发送成功清空——comment_clear_input 此前只置位没人消费（发完
        //    不清，与 Tauri 不一致），一并修掉；
        // 2) 提及插入（候选点击 → 追加 `@名字\u{FFFC} `）
        if let Some(input) = &self.comment_input {
            if self.comment_clear_input {
                self.comment_clear_input = false;
                self.mention_user = None;
                input.update(cx, |state, cx| state.set_value("", window, cx));
            } else if let Some(segment) = self.mention_insert.take() {
                let current = input.read(cx).value().to_string();
                input.update(cx, |state, cx| {
                    state.set_value(format!("{current}{segment}"), window, cx)
                });
            }
        }
        // 下载任务快照刷新（render_stage 是 &self，写不进去；借这里的 &mut）
        if self.downloads_open {
            let sid = self
                .now
                .as_ref()
                .map(|n| n.series_id.clone())
                .unwrap_or_default();
            self.downloads = Arc::new(self.state.queue().of_series(&sid));
            self.done_vids = Arc::new(self.state.queue().completed_vids(&sid));
        }
        // 全屏态图标跟随（Render::render 有 window 可查）
        self.fullscreen = window.is_fullscreen();
        let theme = cx.theme();
        let feed = self.feeds.get(&self.active).expect("源已初始化");
        let notice: Option<String> = if let Some(err) = &feed.failed {
            Some(err.clone())
        } else if feed.loading && self.now.is_none() {
            Some("加载中…".into())
        } else if !feed.loading && feed.items.is_empty() && self.now.is_none() {
            Some("暂无内容".into())
        } else {
            None
        };
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            // 沉浸流锁死在视口内：任何 1px 超高都会冒出页面滚动条并吃掉滚轮
            .overflow_hidden()
            .child(match notice {
                Some(msg) => v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(msg),
                    )
                    .child(
                        Button::new("home-retry")
                            .label("重试")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.load_feed(this.active.clone(), false, cx);
                            })),
                    )
                    .into_any_element(),
                None => self
                    .render_stage(window, cx)
                    .id("home-stage")
                    .on_mouse_move(cx.listener(|this, _: &gpui_kit::MouseMoveEvent, _, cx| {
                        this.wake_controls(cx);
                    }))
                    // 指针离开画面（含鼠标直接甩出窗口）：立即收——控制栏
                    // hover 位一并清掉，不然「指针最后停在控制栏上时甩出窗」
                    // 会永久常显，得点一下别处才收
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.pointer_on_stage = *hovered;
                        if !hovered {
                            this.controls_hovered = false;
                            this.volume_hovered = false;
                            this.volume_popup_hovered = false;
                            this.volume_popup_open = false;
                            if this.controls_until.is_some() {
                                this.controls_until = None;
                            }
                            cx.notify();
                        }
                    }))
                    .on_scroll_wheel(cx.listener(
                        |this, ev: &gpui_kit::ScrollWheelEvent, window, cx| {
                            // 小屏里滚轮不切剧（纯播放器语义，同 ↑↓ 守卫）
                            if this.mini.is_some() {
                                return;
                            }
                            // 滚轮也是「用户在场」（真实输入）：切剧/切集时唤醒
                            // 悬浮层（Tauri wakeChrome 同款，面板守卫之前就亮；
                            // 静默期滚轮同样解除——它不是指针晃动）
                            this.wake_on_input(cx);
                            // 面板开着不抢滚动：选集/下载是 Tauri
                            // seriesPanelOpen/downloading 的显式守卫；评论面板
                            // 在 Tauri 靠 data-wheel-block 拦——面板类统一在
                            // 舞台层守卫（条状区域见各自根节点的滚轮拦截）
                            if this.picker_open || this.comments_panel_open || this.downloads_open {
                                return;
                            }
                            // Tauri 同参：|Δy|<15 忽略；方向同 Tauri
                            //（deltaY>0=下滚=下一部，y<0 即下一部）
                            let y = ev.delta.pixel_delta(window.line_height()).y.to_f64();
                            if y.abs() < 15.0 {
                                return;
                            }
                            // 400ms 冷却吃掉一次惯性滚动的连发。裁决在舞台层
                            // 而不是 step() 里：连播切集路径同样要冷却，而键盘
                            // ↑↓（Tauri 同款）不吃这把滚轮锁
                            if !this.wheel_cooldown_ready() {
                                return;
                            }
                            this.step(if y < 0.0 { 1 } else { -1 }, cx);
                        },
                    ))
                    .into_any_element(),
            })
    }
}
