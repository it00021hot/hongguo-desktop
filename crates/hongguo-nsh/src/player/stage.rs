//! 舞台状态束：沉浸式播放页的全部信号与形态动作（从 `mod.rs` 拆出，
//! 800 行闸门）。`Stage` 只聚合状态与形态切换；编排动作在
//! [`super::actions`]，视图组合在 [`super::view`]。

use nsh_core::ui::prelude::*;

use hongguo_api::detail::RelatedItem;
use hongguo_api::discover::FeedItem;
use hongguo_common::model::{Episode, VideoDefinition};

use crate::i18n::t;
use crate::nav::{Frameless, Incognito, MiniMode};

use super::cadence;
use super::comments::CommentsState;
use super::danmaku::DanmakuState;
use super::info::InfoState;
use super::rail::{RailCtx, RailState};

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
    pub(crate) activity: Stored<std::time::Instant>,
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
    pub(crate) fn new() -> Self {
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
    pub(crate) fn panel_open(&self) -> bool {
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
    pub(crate) fn refit_portrait_if_needed(&self, last: &Stored<f32>) {
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
