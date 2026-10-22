//! 播放域：首页 = 沉浸式播放（对齐 Tauri 壳首页 `player-page.tsx` 的形态）。
//!
//! 视频层占满整个内容区（大窗 contain / 小窗 cover），悬浮层按 Tauri 的
//! 位置叠在画面上：左下信息层、右侧互动栏、底部控制栏、右侧评论面板、
//! 顶部加载细条与错误条。子模块按域拆：`actions`（编排动作）/ `feed`
//! （取流与接口封装）/ `controls` / `mini` / `picker` / `rail` / `info` /
//! `danmaku` / `comments` / `widgets` / `icons`。
//!
//! 交互语义对齐 Tauri：单击画面 = 播放/暂停（并唤醒悬浮层）；悬浮层
//! 静止 3 秒自动收（暂停与任一面板打开时常显）；↑↓ 未连播 = 换剧、
//! 点「观看完整短剧」或自动连播完成本集后 = 切集（Esc 退出连播）；
//! 空格播放暂停、←→ 快退快进 5 秒。滚轮切换 nsh 0.1.1 没有滚轮事件，
//! 暂缺（升级后补）。

pub(crate) mod actions;
pub(crate) mod cadence;
pub(crate) mod comments;
pub(crate) mod comments_view;
pub(crate) mod controls;
pub(crate) mod danmaku;
pub(crate) mod download;
pub(crate) mod feed;
pub(crate) mod history;
pub(crate) mod icons;
pub(crate) mod info;
pub(crate) mod mini;
pub(crate) mod picker;
pub(crate) mod rail;
pub(crate) mod widgets;

use nsh_core::Rect;
use nsh_core::ui::prelude::*;

use hongguo_api::detail::RelatedItem;
use hongguo_api::discover::FeedItem;
use hongguo_common::model::{Episode, VideoDefinition};

use crate::i18n::t;
use crate::nav::{FeedNav, Frameless, Incognito, MiniMode};

use self::comments::CommentsState;
use self::danmaku::DanmakuState;
use self::info::InfoState;
use self::rail::{RailCtx, RailState};

// 编排动作收口：子模块（controls / mini）继续走 `super::动作`。
pub(crate) use actions::{
    fetch_feed, load_related, long_press_rate_off, long_press_rate_on, on_end, pick_episode,
    play_request, replay_episode, seek_ratio, seek_to, send_danmaku, step_chrome, step_episode,
    switch_series, toggle_frameless, toggle_fullscreen, toggle_incognito, toggle_portrait_full,
    wake,
};

/// 推荐流首个 tab（抓包锁定：推荐 16 / 漫剧 36 / 真人剧 39）。
pub(crate) const TAB_FEED: &str = "16";
/// 顶部分类 tab（服务端顶栏下发前/失败时的回落三件套）。
pub(crate) const TABS: [(&str, &str); 3] = [
    ("16", "home.tabFeed"),
    ("36", "home.tabComic"),
    ("39", "home.tabHuman"),
];
/// 倍速档位（与主流播放器一致，Tauri 同表）。
pub(crate) const RATES: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
/// 悬浮层静止多久后收起（B 站方案，Tauri 同款 3 秒）。
const CHROME_HIDE_MS: u64 = 3_000;
/// 光标轮询的「动了」判定阈值（点）：低于它算手抖/采样噪声，不唤醒。
const CURSOR_MOVE_EPS: f32 = 2.0;
/// 纵向滑动切剧的累计位移阈值（点）：低于它算误触，不切。
const PAN_SWITCH_MIN: f32 = 40.0;

/// 舞台状态束（`Clone` 句柄束；闭包按需 clone）。
#[derive(Clone)]
pub(crate) struct Stage {
    // ---- 沉浸流（推荐源）----
    pub items: Signal<Vec<FeedItem>>,
    pub session: Stored<String>,
    pub next_offset: Stored<i64>,
    pub has_more: Stored<bool>,
    pub loading: Signal<bool>,
    pub index: Signal<usize>,
    // ---- 档案 ----
    pub series_id: Signal<String>,
    pub episodes: Signal<Vec<Episode>>,
    /// 当前集的 vid_index（1 基；0 = 未就绪）。
    pub current_index: Signal<u32>,
    /// 相关系列（选集浮层的系列剧 tab）。
    pub related: Signal<Vec<RelatedItem>>,
    // ---- 媒体 ----
    pub source: Signal<String>,
    pub definitions: Signal<Vec<VideoDefinition>>,
    /// 选定档位（0 = 自动，交给 prepare 认缓存/最高档）。
    pub definition: Signal<u32>,
    pub playing: Signal<bool>,
    pub muted: Signal<bool>,
    pub volume: Signal<f32>,
    pub rate_index: Signal<usize>,
    pub buffering: Signal<bool>,
    /// 取流在途（换源请求已发、流未就位）。
    pub preparing: Signal<bool>,
    pub time: Signal<f32>,
    pub duration: Signal<f32>,
    /// 视频宽高比（w/h；竖屏进入用）。
    pub aspect: Signal<f32>,
    pub error: Signal<String>,
    /// 封面占位（当前剧的封面，切剧/起播间隙垫底）。
    pub backdrop: Signal<String>,
    pub player: ElementRef,
    // ---- 舞台几何 ----
    pub stage_w: Signal<f32>,
    pub stage_h: Signal<f32>,
    // ---- 悬浮层（chrome）----
    pub chrome_shown: Signal<bool>,
    activity: Stored<std::time::Instant>,
    /// 连播锁定：true = ↑↓/播完切集，false = ↑↓ 换剧。
    pub binge: Signal<bool>,
    // ---- 面板与弹层 ----
    pub picker_open: Signal<bool>,
    pub rate_menu: Signal<bool>,
    pub def_menu: Signal<bool>,
    pub dm_panel: Signal<bool>,
    pub volume_open: Signal<bool>,
    pub dm_draft: Signal<String>,
    // ---- 域状态 ----
    pub danmaku: DanmakuState,
    pub rail: RailState,
    pub rail_ctx: RailCtx,
    pub info: InfoState,
    pub comments: CommentsState,
    /// 登录态（壳级账号域的镜像；登录面板 / 退出登录翻沿）。
    pub logged_in: Signal<bool>,
    /// 小窗形态（几何在 platform）。
    pub mini: MiniMode,
    /// 无边框播放（chrome 全藏；App 壳按它藏侧栏/顶栏）。
    pub frameless: Frameless,
    /// 隐身模式（离窗透明+暂停，回窗续播）。
    pub incognito: Incognito,
    /// 竖屏形态。
    pub portrait: Signal<bool>,
    /// 竖屏铺满档（platform 静态的镜像，图标态与 refit 档位用）。
    pub portrait_full: Signal<bool>,
    /// 全屏镜像（platform 静态的镜像，全屏按钮的图标态）。
    pub fullscreen: Signal<bool>,
    /// 下载到本地面板。
    pub download_open: Signal<bool>,
    /// 置顶（小窗控制条上也有入口，同一份状态）。
    pub pinned: Signal<bool>,
    /// 长按倍速前的原档位（`Some` = 正在长按快进中）。
    pub long_press_rate: Stored<Option<f32>>,
    /// 竖屏 refit 的上一次比例基线（变化 >1% 才重适配）。
    pub refit_last: Stored<f32>,
    /// 外部播放请求的起始秒（历史/收藏/追更续播）：源 `on_load`（元数据
    /// 就位）时消费并 seek，提前 seek 会打在空流上。
    pub pending_seek: Stored<Option<f64>>,
    /// 观看历史上报节拍的记账器（一次播放生命周期一个）。
    pub history_cadence: Stored<cadence::HistoryCadence>,
}

impl Stage {
    fn new() -> Self {
        let rail = RailState::new();
        let logged_in = crate::account::logged_in();
        let rail_ctx = RailCtx {
            series_id: signal(String::new()),
            bare_vid: signal(String::new()),
            title: signal(String::new()),
            logged_in,
            open_comments: signal(false),
        };
        Self {
            items: signal_with(Vec::new(), SignalEquals::Never),
            session: stored(String::new()),
            next_offset: stored(0),
            has_more: stored(false),
            loading: signal(false),
            index: signal(0),
            series_id: rail_ctx.series_id,
            episodes: signal_with(Vec::new(), SignalEquals::Never),
            current_index: signal(0),
            related: signal_with(Vec::new(), SignalEquals::Never),
            source: signal(String::new()),
            definitions: signal_with(Vec::new(), SignalEquals::Never),
            definition: signal(0),
            playing: signal(true),
            muted: signal(false),
            volume: signal(1.0),
            rate_index: signal(1),
            buffering: signal(true),
            preparing: signal(false),
            time: signal(0.0),
            duration: signal(0.0),
            aspect: signal(9.0 / 16.0),
            error: signal(String::new()),
            backdrop: signal(String::new()),
            player: element_ref(),
            stage_w: signal(0.0),
            stage_h: signal(0.0),
            // 初始可见（Tauri `startVisible` 同款）：起播后 3 秒无人操作
            // 才自动收，别让用户面对一块没有任何入口的黑屏。
            chrome_shown: signal(true),
            activity: stored(std::time::Instant::now()),
            binge: signal(false),
            picker_open: signal(false),
            rate_menu: signal(false),
            def_menu: signal(false),
            dm_panel: signal(false),
            volume_open: signal(false),
            dm_draft: signal(String::new()),
            danmaku: DanmakuState::new(),
            rail,
            rail_ctx,
            info: InfoState::new(),
            comments: CommentsState::new(),
            logged_in,
            mini: MiniMode::from_context(),
            frameless: Frameless::from_context(),
            incognito: Incognito::from_context(),
            portrait: signal(false),
            portrait_full: signal(false),
            fullscreen: signal(false),
            download_open: signal(false),
            pinned: signal(false),
            long_press_rate: stored(None),
            refit_last: stored(0.0),
            pending_seek: stored(None),
            history_cadence: stored(cadence::HistoryCadence::default()),
        }
    }

    /// 画面是否已就位（首帧出画：有流且不在缓冲/取流）。
    pub(crate) fn live(&self) -> bool {
        !self.source.get().is_empty() && !self.buffering.get() && !self.preparing.get()
    }

    /// 中央加载面板的文案。
    pub(crate) fn loading_status(&self) -> String {
        if self.preparing.get() {
            t("player.preparingShort")
        } else if self.buffering.get() {
            t("player.bufferingShort")
        } else {
            t("common.loading")
        }
    }

    /// 任一面板/弹层开着（chrome 常显的条件之一）。
    fn panel_open(&self) -> bool {
        self.picker_open.get()
            || self.rate_menu.get()
            || self.def_menu.get()
            || self.dm_panel.get()
            || self.volume_open.get()
            || self.comments.open.get()
    }

    /// 关掉全部控制栏弹层（点外部/Esc）。
    pub(crate) fn close_popups(&self) {
        batch(|| {
            self.rate_menu.set(false);
            self.def_menu.set(false);
            self.dm_panel.set(false);
            self.volume_open.set(false);
        });
    }

    /// 竖屏用的宽高比（Tauri `portraitAspectOf` 同口径）：竖屏比
    /// （<0.9）与手机形比例取更「长」的一方，横屏/未知回落手机比例
    /// （9:19.5）——按内容包窗会得到矮胖条，不像手机。
    fn portrait_aspect(&self) -> f32 {
        const PORTRAIT_ASPECT_MAX: f32 = 0.9;
        const PHONE_LIKE_ASPECT: f32 = 9.0 / 19.5;
        let video = self.aspect.get();
        if video > 0.0 && video < PORTRAIT_ASPECT_MAX {
            video.min(PHONE_LIKE_ASPECT)
        } else {
            PHONE_LIKE_ASPECT
        }
    }

    /// 进竖屏（宽度按钳制后的视频宽高比收窄）。
    pub(crate) fn enter_portrait(&self) {
        self.portrait.set(true);
        self.portrait_full.set(false);
        let aspect = self.portrait_aspect();
        self.refit_last.replace(aspect);
        crate::platform::enter_portrait(aspect);
    }

    /// 退竖屏回横屏。
    pub(crate) fn exit_portrait(&self) {
        self.portrait.set(false);
        self.portrait_full.set(false);
        crate::platform::exit_portrait();
    }

    /// 竖屏铺满档切换（hgplayer wu：竖屏里全屏语义让位给铺满）。
    pub(crate) fn toggle_portrait_full(&self) {
        if !self.portrait.get() {
            return;
        }
        let next = !self.portrait_full.get();
        self.portrait_full.set(next);
        crate::platform::set_portrait_full(self.portrait_aspect(), next);
    }

    /// 竖屏中切集/换清晰度导致视频比例变化后的重适配（Tauri `Em`：
    /// 变化 >1% 才动；铺满与否跟随当前档）。
    fn refit_portrait_if_needed(&self, last: &Stored<f32>) {
        if !self.portrait.get() {
            return;
        }
        let ar = self.portrait_aspect();
        if (ar - last.with(|v| *v)).abs() <= 0.01 {
            return;
        }
        last.replace(ar);
        crate::platform::set_portrait_full(ar, self.portrait_full.get());
    }

    /// 置顶开关（两处入口共享一份状态）。
    pub(crate) fn toggle_pinned(&self) {
        let next = !self.pinned.get();
        self.pinned.set(next);
        crate::platform::set_always_on_top(next);
    }
}

/// 首页（沉浸式播放）。
#[component]
pub fn PlayerPage() -> impl IntoElement {
    let feed = FeedNav::from_context();
    let s = Stage::new();

    // ---- 顶栏 tab 一变就整源重开（Tauri 的「切 tab 即换片」语义）----
    {
        let s = s.clone();
        let feed_tab = feed.current();
        effect(move || {
            let tab = feed_tab.get();
            batch(|| {
                s.items.set(Vec::new());
                s.session.replace(String::new());
                s.next_offset.replace(0);
                s.has_more.replace(false);
                s.index.set(0);
                s.binge.set(false);
            });
            fetch_feed(&s, &tab, false);
        });
    }

    // ---- 当前集一变：右栏计数 / 左下角标 / 组合键跟上（档案公开计数）----
    {
        let s = s.clone();
        effect(move || {
            let index = s.current_index.get();
            let episode = s
                .episodes
                .with(|list| list.iter().find(|e| e.vid_index == index).cloned());
            if let Some(episode) = episode {
                s.rail.comment_count.set(episode.comment_count);
                s.rail.digg_count.set(episode.digg_count);
                s.rail_ctx.bare_vid.set(episode.vid);
            }
            s.info.vid_index.set(index);
        });
    }

    // ---- 评论区开合：互动栏 💬 触发打开（整页重拉）；面板/键盘关闭后
    //      同步清掉触发位，便于再次打开 ----
    {
        let s = s.clone();
        effect(move || {
            if s.rail_ctx.open_comments.get() {
                s.comments.open(s.rail_ctx.bare_vid, s.series_id);
            }
        });
    }
    {
        let s = s.clone();
        effect(move || {
            if !s.comments.open.get() && s.rail_ctx.open_comments.get() {
                s.rail_ctx.open_comments.set(false);
            }
        });
    }

    // ---- 观看历史上报节拍 / 播放进度落盘（1 秒一拍；暂停沿 force 收尾）----
    history::spawn_heartbeat(s.clone());

    // ---- 外部播放请求（历史/收藏/追更的「继续播放」）：取走并按
    //      （剧, 集号, 起始秒）起播 ----
    {
        let s = s.clone();
        effect(move || {
            if let Some(req) = crate::play_req::take() {
                play_request(&s, req);
            }
        });
    }

    // ---- 悬浮层自动收：播放中、无面板、静止超 3 秒 ----
    {
        let s = s.clone();
        spawn(async move {
            loop {
                timer(std::time::Duration::from_millis(500)).await;
                if !s.playing.get() || s.panel_open() || !s.chrome_shown.get() {
                    continue;
                }
                let idle = s.activity.with(|at| at.elapsed());
                if idle >= std::time::Duration::from_millis(CHROME_HIDE_MS) {
                    s.chrome_shown.set(false);
                    s.close_popups();
                }
            }
        });
    }

    // ---- 鼠标移动唤醒 + 隐身模式：nsh 0.1.1 没有指针移动事件，150ms
    //      轮询光标位置。移动唤醒：在窗口内动了（含刚进窗口）就唤醒——
    //      Tauri mousemove/mouseenter 的同语义替代。隐身（incognito.ts
    //      同款）：开着时离窗 → 整窗透明 + 暂停；回窗 → 重现 + 只续被隐身
    //      欠下的那次播放（用户自己按停的不欠）。 ----
    {
        let s = s.clone();
        let last = stored(None::<(f32, f32)>);
        let was_hidden = stored(false);
        let owed_play = stored(false);
        spawn(async move {
            loop {
                timer(std::time::Duration::from_millis(150)).await;
                let point = crate::platform::cursor_in_window();
                // 隐身分支先行：开着时光标离窗就藏，回窗就还。
                if s.incognito.get() {
                    match point {
                        Some(_) => {
                            if was_hidden.replace(false) {
                                crate::platform::set_window_alpha(1.0);
                                if owed_play.replace(false) {
                                    s.playing.set(true);
                                }
                            }
                        }
                        None => {
                            if !was_hidden.with(|v| *v) {
                                was_hidden.replace(true);
                                // 只在「正在播」时欠：已经暂停着是用户按的，不欠
                                owed_play.replace(s.playing.get());
                                if s.playing.get() {
                                    s.playing.set(false);
                                }
                                crate::platform::set_window_alpha(0.05);
                            }
                        }
                    }
                } else if was_hidden.replace(false) {
                    // 关掉隐身开关与离窗回窗同一条恢复路径。
                    crate::platform::set_window_alpha(1.0);
                    owed_play.replace(false);
                }

                let Some(point) = point else {
                    last.replace(None);
                    continue;
                };
                let moved = last.with(|prev| match *prev {
                    Some(prev) => {
                        (prev.0 - point.0).abs() > CURSOR_MOVE_EPS
                            || (prev.1 - point.1).abs() > CURSOR_MOVE_EPS
                    }
                    None => true,
                });
                last.replace(Some(point));
                if moved {
                    wake(&s);
                }
            }
        });
    }

    // ---- 键盘（nsh 的 When::Auto 保证输入框打字不触发；Esc 不受影响）----
    {
        let s = s.clone();
        on_key("space", move |_| {
            wake(&s);
            s.playing.update(|p| *p = !*p);
        });
    }
    {
        let s = s.clone();
        // 按住 ←→ 连续快退/快进（浏览器 keydown 自然重复，Tauri 同款；
        // nsh 默认按住只发一次，repeat: true 打开连发）。space/↑↓ 不开——
        // 连发会疯狂切播/连跳剧。
        on_key_with(
            "left",
            KeyOptions {
                repeat: true,
                ..Default::default()
            },
            move |_| {
                wake(&s);
                seek_to(&s, (s.time.get() - 5.0).max(0.0));
            },
        );
    }
    {
        let s = s.clone();
        on_key_with(
            "right",
            KeyOptions {
                repeat: true,
                ..Default::default()
            },
            move |_| {
                wake(&s);
                seek_to(&s, s.time.get() + 5.0);
            },
        );
    }
    {
        let s = s.clone();
        on_key("up", move |_| step_chrome(&s, -1));
    }
    {
        let s = s.clone();
        on_key("down", move |_| step_chrome(&s, 1));
    }
    {
        let s = s.clone();
        on_key("escape", move |_| {
            // 链序对齐 Tauri（use-player-interactions Esc）：竖屏铺满最先，
            // 然后面板（评论/选集/弹层），无边框，连播，最后紧凑形态本身。
            if s.comments.open.get() {
                s.comments.open.set(false);
                return;
            }
            if s.panel_open() {
                s.picker_open.set(false);
                s.close_popups();
                return;
            }
            if s.portrait.get() && s.portrait_full.get() {
                s.toggle_portrait_full(); // 铺满回竖条（hgplayer 同序）
                return;
            }
            if s.frameless.get() {
                s.frameless.set(false);
                return;
            }
            if s.binge.get() {
                s.binge.set(false);
                return;
            }
            if s.mini.get() {
                s.mini.set(false);
                return;
            }
            if s.portrait.get() {
                s.exit_portrait();
            }
        });
    }

    // 舞台整体交给 stage_view（收 owned：Rust 2024 的 RPIT 会捕获引用
    // 生命周期，传引用会让返回值借不到 'static）。
    stage_view(s)
}

/// 舞台：视频 + 各悬浮层（叠层顺序 = 子元素顺序，对齐 Tauri 的 z 序）。
fn stage_view(s: Stage) -> impl IntoElement {
    let layout_stage = s.clone();
    let video_stage = s.clone();
    let video_row = s.clone();
    let cover_stage = s.clone();
    let hint_stage = s.clone();
    let binge_stage = s.clone();
    let entry_stage = s.clone();
    let comments_stage = s.clone();
    let mini_stage = s.clone();
    let full_stage = s.clone();
    let loadbar_stage = s.clone();
    let spinner_stage = s.clone();
    let panel_stage = s.clone();
    view()
        .flex_1()
        .relative()
        .overflow_hidden()
        .bg(rgb(0x000000))
        .on_layout(move |r: &Rect| {
            layout_stage.stage_w.set(r.width);
            layout_stage.stage_h.set(r.height);
        })
        // 视频层：铺满舞台，缩放方式随窗态分档（大窗 contain / 紧凑 cover）。
        .child(Each::new(
            move || {
                let url = video_stage.source.get();
                if url.is_empty() {
                    Vec::new()
                } else {
                    vec![url]
                }
            },
            |url: &String| url.clone(),
            move |url: ReadSignal<String>, _| video_layer(video_row.clone(), url),
        ))
        // 封面占位（切剧/起播间隙；整屏背板恒 cover）。
        .child(If::new(
            move || cover_stage.source.get().is_empty() || cover_stage.buffering.get(),
            move || {
                image(move || crate::supply::cover_url(&cover_stage.backdrop.get()))
                    .w_full()
                    .h_full()
                    .resize_mode(ResizeMode::Cover)
            },
        ))
        // 弹幕层。
        .child(s.danmaku.layer())
        // 左下信息层。
        .child(info::info_overlay(s.info, s.chrome_shown))
        // 暂停快捷键提示（紧凑形态不渲染）。
        .child({
            let (playing, mini, portrait, error, stage_h) = (
                hint_stage.playing,
                hint_stage.mini,
                hint_stage.portrait,
                hint_stage.error,
                hint_stage.stage_h,
            );
            info::keyboard_hint(
                move || !playing.get() && !mini.get() && !portrait.get() && error.get().is_empty(),
                stage_h,
            )
        })
        // 连播指示（左上「‹ 第N集」）。
        .child(If::new(
            {
                let s = binge_stage.clone();
                move || s.binge.get() && !s.mini.get() && !s.portrait.get()
            },
            move || {
                let s = binge_stage.clone();
                info::binge_indicator(s.chrome_shown, s.info.vid_index, move || s.binge.set(false))
            },
        ))
        // 「观看完整短剧」横条（连播唯一手动入口；选中后不渲染）。
        .child(If::new(
            {
                let s = entry_stage.clone();
                move || !s.binge.get() && !s.mini.get() && !s.portrait.get()
            },
            move || {
                let s = entry_stage.clone();
                info::binge_entry(s.chrome_shown, s.info.episode_count, move || {
                    s.binge.set(true);
                    wake(&s);
                })
            },
        ))
        // 右侧互动栏。
        .child(rail::rail(s.rail, s.rail_ctx))
        // 评论区。
        .child(If::new(
            {
                let s = comments_stage.clone();
                move || s.comments.open.get() && !s.mini.get() && !s.portrait.get()
            },
            move || {
                let s = comments_stage.clone();
                s.comments
                    .panel(s.rail_ctx.bare_vid, s.series_id, s.logged_in)
            },
        ))
        // 控制栏：小窗/竖屏紧凑条，大屏全量栏。
        .child(If::new(
            {
                let s = mini_stage.clone();
                move || s.mini.get() || s.portrait.get()
            },
            move || {
                let s = mini_stage.clone();
                mini::mini_bar(s.clone(), s.portrait.get())
            },
        ))
        .child(If::new(
            {
                let s = full_stage.clone();
                move || !s.mini.get() && !s.portrait.get()
            },
            move || {
                let s = full_stage.clone();
                controls::bottom_bar(s.clone())
            },
        ))
        // 下载到本地面板（盖在控制栏之上：Sheet 语义）。
        .child(download::sheet(
            s.download_open,
            s.episodes,
            s.current_index,
            s.series_id,
        ))
        // 顶部加载细条 / 中央 spinner / 加载面板 / 错误条。
        .child(info::loadbar(move || {
            loadbar_stage.preparing.get()
                || loadbar_stage.buffering.get()
                || (!loadbar_stage.live() && !loadbar_stage.source.get().is_empty())
        }))
        .child(info::center_spinner(move || {
            spinner_stage.preparing.get()
                || (spinner_stage.buffering.get() && !spinner_stage.source.get().is_empty())
        }))
        .child(If::new(
            {
                let s = panel_stage.clone();
                move || s.source.get().is_empty()
            },
            move || {
                let s = panel_stage.clone();
                let error = s.error;
                let status = {
                    let s = s.clone();
                    move || s.loading_status()
                };
                let retry = move || replay_episode(&s);
                info::loading_panel(error, status, retry)
            },
        ))
        .child(info::error_bar(s.chrome_shown, s.error))
}

/// 视频元素层（按 source 键重建，换集/换清晰度即换元素——
/// `video(url)` 只在构造时读 URL，与 Tauri 的 `<video key>` 同一语义）。
/// 闭包只捕获单个 Copy 信号；要整体 `Stage` 的动作再克隆。
fn video_layer(s: Stage, url: ReadSignal<String>) -> impl IntoElement {
    let playing = s.playing;
    let muted = s.muted;
    let volume = s.volume;
    let rate_index = s.rate_index;
    let mini = s.mini;
    let portrait = s.portrait;
    let load = (
        s.buffering,
        s.duration,
        s.preparing,
        s.aspect,
        s.stage_w,
        s.stage_h,
        s.danmaku,
    );
    let progress = (s.time, s.duration, s.danmaku);
    let end_stage = s.clone();
    let error = (s.buffering, s.preparing, s.error);
    video(url.get().as_str())
        .element_ref(&s.player)
        .paused(move || !playing.get())
        .muted(move || muted.get())
        .volume(move || volume.get().clamp(0.0, 1.0))
        .rate(move || RATES[rate_index.get().min(RATES.len() - 1)])
        .resize_mode(move || {
            if mini.get() || portrait.get() {
                ResizeMode::Cover
            } else {
                ResizeMode::Contain
            }
        })
        // 单击画面 = 播放/暂停（并唤醒悬浮层；Tauri 舞台同语义）。
        .on_press({
            let s = s.clone();
            move |_: &()| {
                wake(&s);
                s.playing.update(|p| *p = !*p);
            }
        })
        // 按下即唤醒（Tauri `onPointerDown` 同款，比抬起触发来得即时）。
        .on_press_in({
            let s = s.clone();
            move |_: &()| wake(&s)
        })
        // 长按 = 2 倍速快进，松手恢复原档（hgplayer/TikTok 同款）。
        // 长按之后 `on_press` 不再触发，单击/长按天然互斥。
        .on_long_press({
            let s = s.clone();
            move |_: &()| long_press_rate_on(&s)
        })
        .on_press_out({
            let s = s.clone();
            move |_: &()| long_press_rate_off(&s)
        })
        // 纵向滑动 = 上一部/下一部剧（连播中切集）：触摸屏/触板手势，
        // 对齐 Tauri 的滚轮换剧语义（横滑失败交还，不干扰单击）。
        // ⚠️ 暂时摘除：0.1.1 上视频的 pan claim 会把整窗触摸吸进视频的
        // 命中链（连控制栏按钮都收不到 press），等 nsh 升级再挂回。
        // .claim_touches(TouchClaim::on_pan(PanActivation::vertical(20.0, 20.0)))
        .on_responder_grant({
            let s = s.clone();
            move |_: &ResponderEvent| wake(&s)
        })
        .on_responder_end({
            let s = s.clone();
            move |e: &ResponderEvent| {
                if e.gesture.dy < -PAN_SWITCH_MIN {
                    step_chrome(&s, -1);
                } else if e.gesture.dy > PAN_SWITCH_MIN {
                    step_chrome(&s, 1);
                }
            }
        })
        .on_load(move |info: &VideoInfo| {
            let (buffering, duration, preparing, aspect, stage_w, stage_h, danmaku) = load;
            batch(|| {
                buffering.set(false);
                duration.set(info.duration);
                preparing.set(false);
                if info.width > 0.0 && info.height > 0.0 {
                    aspect.set(info.width / info.height);
                    // 弹幕带贴画面：contain 模式按实际显示区算。
                    let scale = (stage_w.get() / info.width).min(stage_h.get() / info.height);
                    let band_h = info.height * scale;
                    danmaku.band_h.set(band_h);
                    danmaku.band_top.set((stage_h.get() - band_h) / 2.0);
                }
            });
            // 竖屏中切集/换清晰度导致视频比例变化 → 重适配（Tauri `Em`：
            // 变化 >1% 才动，铺满与否跟随当前档）。
            s.refit_portrait_if_needed(&s.refit_last);
            // 外部播放请求的起始位置（历史/收藏/追更续播）：元数据就位才
            // seek——提前打在空流上会被丢弃。duration 上面刚落到信号，
            // seek 的钳制随之生效。
            if let Some(secs) = s.pending_seek.replace(None) {
                seek_to(&s, secs as f32);
            }
        })
        .on_buffer(move |now: &bool| load.0.set(*now))
        .on_progress(move |p: &VideoProgress| {
            let (time, duration, danmaku) = progress;
            batch(|| {
                time.set(p.current_time);
                duration.set(p.duration);
                danmaku.on_progress(p.current_time);
            });
        })
        .on_end(move |_: &()| on_end(&end_stage))
        .on_error(move |message: &str| {
            let (buffering, preparing, error) = error;
            log::warn!("[Player] 播放错误: {message}");
            batch(|| {
                buffering.set(false);
                preparing.set(false);
                error.set(format!("{}：{message}", t("player.onlineInterrupted")));
            });
        })
        .w_full()
        .h_full()
}
