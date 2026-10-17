//! 壳窗口：持久侧栏导航 + 顶栏 + 内容区视图切换（Tauri `app-shell` /
//! `app-sidebar` 的 GPUI 对齐形态）。
//!
//! 侧栏目的地对齐 Tauri 客户端的信息架构（13 项，逐页接真数据）。
//! 可折叠：展开 224px 图标+文字，折叠 56px 纯图标（tooltip 出标签，
//! Tauri collapsed 同款）。贴底账户区（登录面板复用首页的）+ 折叠钮。
//!
//! 顶栏自绘（Tauri `app-shell` 44px 薄行对齐）：中部是当前页面分类 tab 的
//! 居中槽（Tauri `TopBarTabsPortal` 同位；首页/排行榜/新剧三页自带），
//! 右端置顶 / 外观 / 窗口钮，空白带按住拖窗、双击最大化。
//! 为什么不用组件库 TitleBar：见 [`Shell::render_toolbar`] 头注释。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, InteractiveElementExt as _, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ElementId, Entity, ImageSource, ObjectFit, RenderImage, SharedString, Window, div, img,
};
use hongguo_service::app_state::AppStateInner;
use hongguo_service::events::ServiceEvent;

use crate::detail::{DetailEvent, DetailView};
use crate::home::HomeEvent;
use crate::home::HomeView;
use crate::pages::browse::{BrowseEvent, BrowseView};
use crate::pages::collections::{CollectionEvent, CollectionView};
use crate::pages::creator::{CreatorEvent, CreatorView};
use crate::pages::follows::{FollowsEvent, FollowsView};
use crate::pages::history::{HistoryEvent, HistoryView};
use crate::pages::liked::{LikedEvent, LikedView};
use crate::pages::merge::MergeView;
use crate::pages::new_drama::{NewDramaEvent, NewDramaView};
use crate::pages::rank::{RankEvent, RankView};
use crate::pages::reservations::{ReservationEvent, ReservationView};
use crate::pages::settings::{SettingsEvent, SettingsView};
use crate::pages::storage::StorageView;
use crate::pages::tasks::TasksView;

/// 侧栏导航目的地（与 Tauri `app-sidebar.tsx` 的 NAV_ITEMS 逐项同序）。
/// 播放不在列：它由各页「播放/继续播放」进入，首页本身就是沉浸流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dest {
    Home,
    Browse,
    Rank,
    New,
    History,
    Collections,
    Liked,
    Follows,
    Reservations,
    Tasks,
    Merge,
    Storage,
    Settings,
    /// 应用内详情页（不在侧栏：播放页点剧名 / 播放入口进入）。
    Detail,
    /// 创作者主页（不在侧栏：详情页出品方/演职员芯片进入）。
    Creator,
}

impl Dest {
    const ALL: [Dest; 13] = [
        Dest::Home,
        Dest::Browse,
        Dest::Rank,
        Dest::New,
        Dest::History,
        Dest::Collections,
        Dest::Liked,
        Dest::Follows,
        Dest::Reservations,
        Dest::Tasks,
        Dest::Merge,
        Dest::Storage,
        Dest::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Dest::Home => "首页",
            Dest::Browse => "找剧",
            Dest::Rank => "排行榜",
            Dest::New => "新剧",
            Dest::History => "历史",
            Dest::Collections => "收藏",
            Dest::Liked => "点赞",
            Dest::Follows => "追更",
            Dest::Reservations => "预约",
            Dest::Tasks => "下载管理",
            Dest::Merge => "一键合并",
            Dest::Storage => "磁盘清理",
            Dest::Settings => "设置",
            // 不在侧栏（播放页进入），仅兜匹配
            Dest::Detail => "详情",
            Dest::Creator => "创作者",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Dest::Home => IconName::Flame,
            Dest::Browse => IconName::Compass,
            Dest::Rank => IconName::Trophy,
            Dest::New => IconName::Sparkles,
            Dest::History => IconName::ClockArrowDown,
            Dest::Collections => IconName::Star,
            Dest::Liked => IconName::Heart,
            Dest::Follows => IconName::Tv,
            Dest::Reservations => IconName::BellRing,
            Dest::Tasks => IconName::ListChecks,
            Dest::Merge => IconName::Combine,
            Dest::Storage => IconName::HardDrive,
            Dest::Settings => IconName::Settings,
            Dest::Detail => IconName::Info,
            Dest::Creator => IconName::UserRound,
        }
    }

    /// 取证开关取值 → 目的地（`HONGGUO_PAGE=browse|new|…`，与 HONGGUO_RANK
    /// 同族：无人值守点检任一页面）。
    fn from_key(key: &str) -> Option<Dest> {
        match key {
            "home" => Some(Dest::Home),
            "browse" => Some(Dest::Browse),
            "rank" => Some(Dest::Rank),
            "new" => Some(Dest::New),
            "history" => Some(Dest::History),
            "collections" => Some(Dest::Collections),
            "liked" => Some(Dest::Liked),
            "follows" => Some(Dest::Follows),
            "reservations" => Some(Dest::Reservations),
            "tasks" => Some(Dest::Tasks),
            "merge" => Some(Dest::Merge),
            "storage" => Some(Dest::Storage),
            "settings" => Some(Dest::Settings),
            _ => None,
        }
    }

    /// 目的地 → 壳内视图（13 页 + 详情全部实现，无空态兜底）。
    fn view(self) -> View {
        match self {
            Dest::Home => View::Home,
            Dest::Rank => View::Rank,
            Dest::Browse => View::Browse,
            Dest::New => View::New,
            Dest::Liked => View::Liked,
            Dest::Collections => View::Collections,
            Dest::Follows => View::Follows,
            Dest::History => View::History,
            Dest::Reservations => View::Reservations,
            Dest::Tasks => View::Tasks,
            Dest::Settings => View::Settings,
            Dest::Storage => View::Storage,
            Dest::Merge => View::Merge,
            Dest::Detail => View::Detail,
            Dest::Creator => View::Creator,
        }
    }
}

/// 壳内视图（Dest::view 的落点；Detail 不在侧栏，由播放页点剧名进入）。
enum View {
    Home,
    Rank,
    Browse,
    New,
    Liked,
    Collections,
    Follows,
    History,
    Reservations,
    Tasks,
    Settings,
    Storage,
    Merge,
    Detail,
    Creator,
}

/// 壳窗口实体：持有各视图实体，切换当前目的地。服务面由视图各自持有。
pub struct Shell {
    dest: Dest,
    state: Arc<AppStateInner>,
    sidebar_collapsed: bool,
    /// 主题偏好（auto 时随系统外观重算；设置页改动即存即生效）。
    theme: crate::ui_prefs::ThemePref,
    feed: Entity<HomeView>,
    rank: Entity<RankView>,
    browse: Entity<BrowseView>,
    new_drama: Entity<NewDramaView>,
    liked: Entity<LikedView>,
    collections: Entity<CollectionView>,
    follows: Entity<FollowsView>,
    history: Entity<HistoryView>,
    reservations: Entity<ReservationView>,
    tasks: Entity<TasksView>,
    settings_page: Entity<SettingsView>,
    storage: Entity<StorageView>,
    merge: Entity<MergeView>,
    creator: Entity<CreatorView>,
    detail: Option<Entity<DetailView>>,
    /// 详情页当前加载的剧（同剧重复点击不重载）。
    detail_series: Option<String>,
    /// 侧栏追更未读角标（本地通知未读数；未登录恒 0）。
    follow_unread: usize,
    /// 退出确认框已经弹过（Tauri CloseRequested 的「已问过」闸门）：再收到
    /// 关闭请求就直接放行——宁可少问一句，绝不把窗口变成关不掉的
    ///（Tauri「前端未就绪就放行」同精神）。
    exit_asked: bool,
    /// 账号头像（avatar_url 拉取；账号变化时重拉，登出清空）。
    account_avatar: Option<Arc<RenderImage>>,
    account_avatar_url: Option<String>,
    account_avatar_pending: bool,
    /// 顶栏拖窗的按下起点：移动超过阈值才 `start_window_move`（按钮上的
    /// 轻微抖动不误起拖）。
    toolbar_drag: Option<gpui_kit::Point<gpui_kit::Pixels>>,
}

impl Shell {
    pub fn new(state: Arc<AppStateInner>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let feed = cx.new(|cx| HomeView::new(state.clone(), window, cx));
        let rank = cx.new(|_cx| RankView::new(state.clone()));
        let browse = cx.new(|cx| BrowseView::new(state.clone(), window, cx));
        let new_drama = cx.new(|_cx| NewDramaView::new(state.clone()));
        let liked = cx.new(|cx| LikedView::new(state.clone(), window, cx));
        let collections = cx.new(|cx| CollectionView::new(state.clone(), window, cx));
        let history = cx.new(|cx| HistoryView::new(state.clone(), window, cx));
        let reservations = cx.new(|cx| ReservationView::new(state.clone(), window, cx));
        let tasks = cx.new(|cx| TasksView::new(state.clone(), cx));
        let settings_page = cx.new(|cx| SettingsView::new(state.clone(), window, cx));
        let storage = cx.new(|cx| StorageView::new(state.clone(), window, cx));
        let merge = cx.new(|cx| MergeView::new(state.clone(), window, cx));
        let follows = cx.new(|cx| FollowsView::new(state.clone(), window, cx));
        let creator = cx.new(|_cx| CreatorView::new(state.clone()));
        // 播放页 → 详情页：HomeView 发事件，壳切目的地（页面间不互相持有）
        cx.subscribe(&feed, |this, _, event: &HomeEvent, cx| match event {
            HomeEvent::OpenDetail(series_id) => this.open_detail(series_id.clone(), cx),
            // 登录/登出：头像与追更角标跟着账号走
            HomeEvent::AccountChanged => this.on_account_changed(cx),
            // 互动登录墙：切设置页（账号卡登录入口）
            HomeEvent::OpenSettings => this.enter_dest(Dest::Settings, cx),
        })
        .detach();
        // 点赞/收藏卡「继续播放」：壳切回首页沉浸流起播（与详情页同通道）
        cx.subscribe(&liked, |this, _, event: &LikedEvent, cx| {
            let LikedEvent::Play {
                series_id,
                vid_index,
                start_secs,
            } = event.clone();
            this.start_play(series_id, vid_index, start_secs, cx);
        })
        .detach();
        cx.subscribe(&browse, |this, _, event: &BrowseEvent, cx| {
            let BrowseEvent::Play {
                series_id,
                vid_index,
                start_secs,
            } = event.clone();
            this.start_play(series_id, vid_index, start_secs, cx);
        })
        .detach();
        cx.subscribe(
            &new_drama,
            |this, _, event: &NewDramaEvent, cx| match event.clone() {
                NewDramaEvent::Play {
                    series_id,
                    vid_index,
                    start_secs,
                } => this.start_play(series_id, vid_index, start_secs, cx),
                NewDramaEvent::OpenDetail { series_id, prefill } => {
                    this.open_detail_with_prefill(series_id, Some(prefill), cx);
                }
            },
        )
        .detach();
        cx.subscribe(&rank, |this, _, event: &RankEvent, cx| {
            match event.clone() {
                RankEvent::Play {
                    series_id,
                    vid_index,
                    start_secs,
                } => this.start_play(series_id, vid_index, start_secs, cx),
                RankEvent::OpenDetail { series_id, prefill } => {
                    this.open_detail_with_prefill(series_id, Some(prefill), cx);
                }
            }
        })
        .detach();
        cx.subscribe(&collections, |this, _, event: &CollectionEvent, cx| {
            let CollectionEvent::Play {
                series_id,
                vid_index,
                start_secs,
            } = event.clone();
            this.start_play(series_id, vid_index, start_secs, cx);
        })
        .detach();
        cx.subscribe(&history, |this, _, event: &HistoryEvent, cx| {
            let HistoryEvent::Play {
                series_id,
                vid_index,
                start_secs,
            } = event.clone();
            this.start_play(series_id, vid_index, start_secs, cx);
        })
        .detach();
        cx.subscribe(
            &reservations,
            |this, _, event: &ReservationEvent, cx| match event.clone() {
                ReservationEvent::Play {
                    series_id,
                    vid_index,
                    start_secs,
                } => this.start_play(series_id, vid_index, start_secs, cx),
                ReservationEvent::OpenDetail { series_id, prefill } => {
                    this.open_detail_with_prefill(series_id, Some(prefill), cx);
                }
                ReservationEvent::Login => {
                    // 切回首页开登录面板（Tauri 弹 LoginDialog 的同位入口）；
                    // 回首页即重挂载语义，续播跟着走
                    this.dest = Dest::Home;
                    this.feed.update(cx, |home, cx| {
                        home.resume_if_idle(cx);
                        home.toggle_login_panel(cx);
                    });
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &settings_page,
            |this, _, event: &SettingsEvent, cx| match event {
                SettingsEvent::Login => {
                    this.dest = Dest::Home;
                    this.feed.update(cx, |home, cx| {
                        home.resume_if_idle(cx);
                        home.toggle_login_panel(cx);
                    });
                    cx.notify();
                }
                SettingsEvent::ShortcutKeys(keys) => {
                    let keys = keys.clone();
                    this.feed
                        .update(cx, |home, cx| home.reload_shortcut_keys(keys, cx));
                }
                SettingsEvent::ThemeChanged(pref) => {
                    // auto 观察者按这个字段判据；显式档位不需观察
                    this.theme = *pref;
                }
            },
        )
        .detach();
        cx.subscribe(&follows, |this, _, event: &FollowsEvent, cx| {
            match event.clone() {
                FollowsEvent::Play {
                    series_id,
                    vid_index,
                    start_secs,
                } => this.start_play(series_id, vid_index, start_secs, cx),
                FollowsEvent::OpenDetail { series_id, prefill } => {
                    this.open_detail_with_prefill(series_id, Some(prefill), cx);
                }
                FollowsEvent::Login => {
                    // 切回首页开登录面板（Tauri 弹 LoginDialog 的同位入口）
                    this.dest = Dest::Home;
                    this.feed.update(cx, |home, cx| {
                        home.resume_if_idle(cx);
                        home.toggle_login_panel(cx);
                    });
                    cx.notify();
                }
                FollowsEvent::NoticesChanged => this.refresh_follow_unread(cx),
            }
        })
        .detach();
        cx.subscribe(&creator, |this, _, event: &CreatorEvent, cx| {
            match event.clone() {
                CreatorEvent::Play {
                    series_id,
                    vid_index,
                    start_secs,
                } => this.start_play(series_id, vid_index, start_secs, cx),
                CreatorEvent::Back => this.close_creator(cx),
            }
        })
        .detach();
        // 壳级服务事件：独立订阅一条广播（与 HomeView 的通道互不抢）。
        // 追更提醒到达 → 侧栏角标刷新 + 追更页列表翻面。
        let mut shell_events = crate::events::subscribe(&state);
        cx.spawn(async move |this, cx| {
            while let Some(event) = shell_events.recv().await {
                let ok = this
                    .update(cx, |shell, cx| shell.on_service_event(event, cx))
                    .is_ok();
                if !ok {
                    break;
                }
            }
        })
        .detach();
        // 取证开关：HONGGUO_RANK=1 直接开在排行榜页（无头验证封面用）；
        // HONGGUO_TAB=36/39 直接开在漫剧/真人源（tab 数据链取证用）；
        // HONGGUO_PAGE=browse|new|history|… 直接开在对应目的地（逐页点检）；
        // HONGGUO_DETAIL=<site> / HONGGUO_CREATOR=<uid> 直接开在详情/创作者
        // 页（这两页不在侧栏，无开关时无人值守到不了）。
        // 预览进程有自己的目的地种子，不吃这些环境变量（可能残留）
        let page_start = (!crate::preview::is_active())
            .then(|| std::env::var("HONGGUO_PAGE").ok())
            .flatten()
            .and_then(|v| Dest::from_key(&v));
        let tab_source = (!crate::preview::is_active())
            .then(|| std::env::var("HONGGUO_TAB").ok())
            .flatten()
            .and_then(|v| match v.as_str() {
                "36" | "39" => Some(crate::home::Source::new(&v)),
                _ => None,
            });
        let dest = if !crate::preview::is_active() && std::env::var("HONGGUO_RANK").is_ok() {
            rank.update(cx, RankView::load);
            Dest::Rank
        } else if let Some(page) = page_start {
            page
        } else {
            if let Some(source) = tab_source {
                feed.update(cx, |home, cx| home.switch_source(source, cx));
            }
            Dest::Home
        };
        // UI 偏好盘上有就按盘上（Tauri 默认折叠、展开后持久化记住；主题默认
        // auto，开窗前的应用见 main.rs）
        let ui_prefs = crate::ui_prefs::load();
        let mut shell = Self {
            dest,
            state,
            sidebar_collapsed: ui_prefs.sidebar_collapsed,
            theme: ui_prefs.theme,
            feed,
            rank,
            browse,
            new_drama,
            liked,
            collections,
            follows,
            history,
            reservations,
            tasks,
            settings_page,
            storage,
            merge,
            creator,
            detail: None,
            detail_series: None,
            follow_unread: 0,
            exit_asked: false,
            account_avatar: None,
            account_avatar_url: None,
            account_avatar_pending: false,
            toolbar_drag: None,
        };
        if let Some(page) = page_start {
            shell.enter_dest(page, cx);
        }
        if !crate::preview::is_active() {
            if let Ok(sid) = std::env::var("HONGGUO_DETAIL")
                && !sid.is_empty()
            {
                shell.open_detail(sid, cx);
                // 取证：直接落在指定下半区 tab（默认选集）——剧评发评框/
                // 相关推荐的无头截图入口
                if let Ok(tab) = std::env::var("HONGGUO_DETAIL_TAB")
                    && let Some(detail) = &shell.detail
                {
                    let tab = match tab.as_str() {
                        "reviews" => crate::detail::DetailTab::Reviews,
                        "related" => crate::detail::DetailTab::Related,
                        _ => crate::detail::DetailTab::Episodes,
                    };
                    detail.update(cx, |view, cx| view.select_tab(tab, cx));
                }
            } else if let Ok(uid) = std::env::var("HONGGUO_CREATOR")
                && !uid.is_empty()
            {
                shell.creator.update(cx, |view, cx| view.load(uid, cx));
                shell.dest = Dest::Creator;
                cx.notify();
            }
        }
        shell.refresh_follow_unread(cx);
        shell.ensure_account_avatar(cx);
        // 老板键轮询：全局快捷键注册在 win32 自建的消息窗口上，WM_HOTKEY
        // 只能在轮询里递进 GPUI（窗口过程跑在主线程消息循环里，拿不到
        // GPUI 上下文）。窗口过程是「按下沿置位」，100ms 一拍不会漏。
        #[cfg(target_os = "windows")]
        if let Some(hwnd) = crate::win32::hwnd_of(window) {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(100))
                        .await;
                    // 第二次启动请求前置窗口（单实例；Tauri bring_back_active_window
                    // 同款：显示 + 还原最小化 + 聚焦）
                    if crate::single_instance::take_focus_request() {
                        if crate::win32::is_iconic(hwnd) {
                            crate::win32::show_window(hwnd, crate::win32::SW_RESTORE);
                        }
                        crate::win32::show_foreground(hwnd);
                        log::info!(
                            "[Shell] 第二次启动请求：窗口已带到前台（可见={}）",
                            crate::win32::is_window_visible(hwnd)
                        );
                    }
                    if !crate::boss_key::take_fired() {
                        continue;
                    }
                    // 先查可见性：ShowWindow 放实体更新之外——它同步派发
                    // WM_SHOWWINDOW/WM_ACTIVATE，gpui 的窗口过程要更新窗口
                    // 状态，在 update 里调用会撞借用（实测日志刷
                    // 「RefCell already borrowed」六连）。
                    let visible = crate::win32::is_window_visible(hwnd);
                    let ok = this
                        .update(cx, |shell, cx| {
                            shell.boss_key_playback(!visible, cx);
                        })
                        .is_ok();
                    if !ok {
                        return;
                    }
                    if visible {
                        crate::win32::hide(hwnd);
                    } else {
                        crate::win32::show_foreground(hwnd);
                    }
                    log::info!(
                        "[BossKey] 老板键切换：窗口{}",
                        if visible { "隐藏" } else { "显示" }
                    );
                }
            })
            .detach();
        }
        // 跟随系统：系统外观切换（Windows 主题 / macOS 外观）时重算主题档。
        // 显式 light/dark 不受影响（钉住的外观不会触发这个回调）。
        cx.observe_window_appearance(window, |this, window, cx| {
            if this.theme == crate::ui_prefs::ThemePref::Auto {
                let mode = this.theme.resolve(window.appearance());
                gpui_kit::component::Theme::change(mode, None, cx);
            }
        })
        .detach();
        shell
    }

    /// 老板键触发的播放处置（Tauri 前端 `boss-key-hidden` 事件同款）：
    /// 隐藏即暂停（声音不漏）、恢复时把暂停着的续上。窗口藏显由调用方在
    /// 实体更新之外做（见 `Shell::new` 的轮询注释）。
    #[cfg(target_os = "windows")]
    fn boss_key_playback(&self, hidden: bool, cx: &mut Context<Self>) {
        self.feed
            .update(cx, |home, cx| home.boss_key_hidden(hidden, cx));
    }

    /// 切换目的地并触发进入钩子（侧栏点击与 `HONGGUO_PAGE` 取证开关共用）。
    /// 首页播放器的生命周期跟着走（Tauri 路由卸载/重挂载的同语义）：离开
    /// 首页停播落进度，回首页从落盘进度接上。
    fn enter_dest(&mut self, dest: Dest, cx: &mut Context<Self>) {
        if dest == Dest::Home {
            self.feed.update(cx, |home, cx| home.resume_if_idle(cx));
        } else {
            self.feed.update(cx, |home, cx| home.release_for_leave(cx));
        }
        self.dest = dest;
        match dest {
            Dest::Rank => self.rank.update(cx, RankView::load),
            Dest::Browse => self.browse.update(cx, BrowseView::load),
            Dest::New => self.new_drama.update(cx, NewDramaView::load),
            Dest::Liked => self.liked.update(cx, LikedView::load),
            Dest::Collections => self.collections.update(cx, CollectionView::load),
            Dest::Follows => {
                self.follows.update(cx, FollowsView::load);
                self.refresh_follow_unread(cx);
            }
            Dest::History => self.history.update(cx, HistoryView::load),
            Dest::Reservations => self.reservations.update(cx, ReservationView::load),
            Dest::Tasks => self.tasks.update(cx, TasksView::load),
            Dest::Settings => self.settings_page.update(cx, SettingsView::load),
            Dest::Storage => self.storage.update(cx, StorageView::load),
            Dest::Merge => self.merge.update(cx, MergeView::load),
            _ => {}
        }
        cx.notify();
    }

    /// 追更未读角标刷新（本地通知未读数；未登录恒 0，Tauri 单入口口径）。
    fn refresh_follow_unread(&mut self, cx: &mut Context<Self>) {
        let count = if self.state.settings().account.is_some() {
            hongguo_service::follow_service::notices(&self.state)
                .map(|ns| ns.iter().filter(|n| !n.read).count())
                .unwrap_or(0)
        } else {
            0
        };
        self.follow_unread = count;
        cx.notify();
    }

    /// 壳级服务事件消费（独立通道；与 HomeView 的事件循环互不影响）。
    fn on_service_event(&mut self, event: ServiceEvent, cx: &mut Context<Self>) {
        if let ServiceEvent::FollowNotices(_) = event {
            // 新提醒：角标刷新 + 追更页列表翻面（数据在本地库，一次小读；
            // 页面在不在前台都刷新，回到页面即最新）
            self.refresh_follow_unread(cx);
            self.follows
                .update(cx, |view, cx| view.on_notices_arrived(cx));
        }
    }

    /// 账号变化（登录/登出）：头像重拉 + 追更角标重算。
    fn on_account_changed(&mut self, cx: &mut Context<Self>) {
        self.account_avatar_url = None;
        self.ensure_account_avatar(cx);
        self.refresh_follow_unread(cx);
    }

    /// 账号头像加载：账号在且 URL 没拉过才拉一次（登录/换头像后重拉，
    /// 登出即清）；下载失败留空退回通用图标。
    fn ensure_account_avatar(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.state.settings().account else {
            self.account_avatar = None;
            self.account_avatar_url = None;
            return;
        };
        let url = account.avatar_url;
        if url.is_empty() || self.account_avatar_url.as_deref() == Some(url.as_str()) {
            return;
        }
        if self.account_avatar_pending {
            return;
        }
        self.account_avatar_pending = true;
        let rx = crate::cover_img::fetch(url.clone());
        cx.spawn(async move |this, cx| {
            let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |shell, cx| {
                shell.account_avatar_pending = false;
                if let Ok(bytes) = bytes
                    && let Ok(decoded) = image::load_from_memory(&bytes)
                {
                    shell.account_avatar = Some(Arc::new(crate::image_upload::render_image(
                        decoded.to_rgba8(),
                    )));
                    shell.account_avatar_url = Some(url);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 侧栏折叠态落盘（`ui_prefs` 读-改-写，Tauri zustand persist 同语义）。
    fn persist_sidebar_collapsed(&self) {
        let mut prefs = crate::ui_prefs::load();
        prefs.sidebar_collapsed = self.sidebar_collapsed;
        crate::ui_prefs::save(prefs);
    }

    /// 页面「继续播放」的统一入口：切回首页沉浸流起播（详情页播放同通道，
    /// 页面间不互相持有）。
    fn start_play(
        &mut self,
        series_id: String,
        vid_index: u32,
        start_secs: f64,
        cx: &mut Context<Self>,
    ) {
        self.dest = Dest::Home;
        self.feed.update(cx, |home, cx| {
            home.play_from_detail(series_id, vid_index, start_secs, cx)
        });
        cx.notify();
    }

    /// 打开应用内详情页（播放页点剧名）：同剧重复点击不重载。
    fn open_detail(&mut self, series_id: String, cx: &mut Context<Self>) {
        self.open_detail_with_prefill(series_id, None, cx);
    }

    /// 打开详情页（可带来源页 prefill：未上线剧走「即将上线」降级视图）。
    pub(crate) fn open_detail_with_prefill(
        &mut self,
        series_id: String,
        prefill: Option<crate::detail::DetailPrefill>,
        cx: &mut Context<Self>,
    ) {
        if self.dest == Dest::Detail && self.detail_series.as_deref() == Some(&series_id) {
            return;
        }
        // 进详情页 = 离开首页沉浸流（Tauri /detail 是独立路由，进入即卸载
        // 首页）：停播落进度。从别的页进来时引擎多半已被 enter_dest 释放，
        // 这里的无条件调用只是兜「首页还没切走」的路径。
        self.feed.update(cx, |home, cx| home.release_for_leave(cx));
        let view = match &self.detail {
            Some(v) => v.clone(),
            None => {
                let v = cx.new(|cx| DetailView::new(self.state.clone(), cx));
                // 详情页事件（返回/播放）→ 壳裁决目的地与起播
                cx.subscribe(&v, |this, _, event: &DetailEvent, cx| {
                    this.on_detail_event(event.clone(), cx);
                })
                .detach();
                self.detail = Some(v.clone());
                v
            }
        };
        view.update(cx, |detail, cx| {
            detail.load_with_prefill(series_id.clone(), prefill, cx)
        });
        self.detail_series = Some(series_id);
        self.dest = Dest::Detail;
        cx.notify();
    }

    /// 首页当前 tab 源（预览交互探针读，断言切源生效）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn active_source(&self, cx: &App) -> crate::home::Source {
        self.feed.read(cx).active_source()
    }

    /// 详情页的返回/播放入口（DetailView 发事件，壳裁决目的地）。
    fn on_detail_event(&mut self, event: DetailEvent, cx: &mut Context<Self>) {
        match event {
            DetailEvent::Back => {
                self.dest = Dest::Home;
                // 回首页续播（Tauri 返回重挂载「挂载默认开闸」同语义）
                self.feed.update(cx, |home, cx| home.resume_if_idle(cx));
                cx.notify();
            }
            DetailEvent::Play {
                series_id,
                vid_index,
                start_secs,
            } => {
                self.dest = Dest::Home;
                self.feed.update(cx, |home, cx| {
                    home.play_from_detail(series_id, vid_index, start_secs, cx)
                });
                cx.notify();
            }
            // 出品方/演职员芯片 → 创作者主页（详情实体保留，返回原地恢复）
            DetailEvent::OpenCreator(user_id) => {
                self.creator.update(cx, |view, cx| view.load(user_id, cx));
                self.dest = Dest::Creator;
                cx.notify();
            }
        }
    }

    /// 创作者主页返回：回来源详情页（实体保留原样）；没有详情来源时回首页
    /// 续播（与详情返回同语义）。
    fn close_creator(&mut self, cx: &mut Context<Self>) {
        if self.detail.is_some() && self.detail_series.is_some() {
            self.dest = Dest::Detail;
        } else {
            self.dest = Dest::Home;
            self.feed.update(cx, |home, cx| home.resume_if_idle(cx));
        }
        cx.notify();
    }

    /// 给 main 的启动回调：把窗口提到前台。
    pub fn activate(&self, cx: &mut App) {
        cx.activate(true);
    }

    /// 取证辅助（screenshot feature）：暂停播放 + 拉长控制条可见窗，让
    /// 无人值守抓帧能拍到控制条/互动栏。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_pause_player(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.debug_pause_player(cx));
    }

    /// 取证辅助（screenshot feature）：打开下载面板。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_downloads(&self, cx: &mut App) {
        self.feed
            .update(cx, |home, cx| home.debug_open_downloads(cx));
    }

    /// 取证辅助（screenshot feature）：打开选集浮层系列剧 tab。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_picker(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.debug_open_picker(cx));
    }

    /// 取证辅助（screenshot feature）：打开评论面板与表情选择器。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_comments(&self, cx: &mut App) {
        self.feed
            .update(cx, |home, cx| home.debug_open_comments(cx));
    }

    /// 取证辅助（screenshot feature）：打开评论面板并派发 @提及弹层。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_mention(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.debug_open_mention(cx));
    }

    /// 取证辅助（screenshot feature）：打开登录面板。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_login(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.debug_open_login(cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=suggest`，配
    /// `HONGGUO_PAGE=browse`）：往找剧页搜索框灌关键词——联想走真实接口，
    /// 抓帧拍的是真实下拉列表（含封面缩略图与命中高亮）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_fill_browse_keyword(&self, window: &mut Window, cx: &mut App) {
        self.browse
            .update(cx, |view, cx| view.debug_fill_keyword("斗罗", window, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=boss-set`）：
    /// 走真实保存路径写老板键（注册 → 落库；冲突则拦截）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_boss_key(&self, window: &mut Window, cx: &mut App) {
        let spec = std::env::var("HONGGUO_BOSS_KEY").unwrap_or_default();
        self.settings_page
            .update(cx, |view, cx| view.debug_set_boss_key(&spec, window, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=portrait`）：
    /// 进竖屏（几何按当前在播档位收窄），抓帧拍竖条形态。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_enter_portrait(&self, window: &mut Window, cx: &mut App) {
        self.feed
            .update(cx, |home, cx| home.enter_portrait(window, cx));
        // 顺带逼一次重适配路径（清零「上次比例」→ 下一帧 render 里的
        // refit_portrait 必跑），验「render 期派发窗口操作」这条不炸
        self.feed
            .update(cx, |home, _| home.debug_force_portrait_refit());
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=portrait-full`）：
    /// 进竖屏后切铺满档（验撑满工作区高的几何与铺满态按钮）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_portrait_full(&self, window: &mut Window, cx: &mut App) {
        self.feed.update(cx, |home, cx| {
            home.enter_portrait(window, cx);
            home.toggle_portrait_full(window, cx);
        });
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=auto-delete`）：
    /// 打开「看完自动删除」（走真实保存路径），供播放到底的行为取证。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_auto_delete(&self, window: &mut Window, cx: &mut App) {
        self.settings_page
            .update(cx, |view, cx| view.debug_set_auto_delete(window, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=next-series`）：
    /// 切到信息流下一条（换剧）——验切剧走封面、不留上一部的定格帧。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_next_series(&mut self, cx: &mut App) {
        self.dest = Dest::Home;
        self.feed.update(cx, |home, cx| home.step(1, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=mini`）：
    /// 进小屏（480×270），验铺满裁切与小屏布局。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_enter_mini(&self, window: &mut Window, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.enter_mini(window, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=related-upcoming`，
    /// 配 `HONGGUO_DETAIL=<sid>`）：在当前详情页走一次「未上线相关推荐」点击
    /// 路径，验 prefill 兜住「解析失败 → 即将上线降级视图」。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_upcoming_related(&self, cx: &mut App) {
        if let Some(detail) = &self.detail {
            detail.update(cx, |view, cx| view.debug_open_upcoming_related(cx));
        } else {
            log::warn!("[Shot] related-upcoming 需要 HONGGUO_DETAIL=<sid> 先打开详情页");
        }
    }

    /// 取证辅助（screenshot feature）：按 `HONGGUO_PAGE` 的键切目的地
    ///（验「去播放再回来」的浏览位置保留）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_switch_dest(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(dest) = Dest::from_key(key) {
            self.enter_dest(dest, cx);
        }
    }

    /// 预览种子分发（screenshot feature）：rank 场景切到排行榜并灌假榜单，
    /// 其余全走首页。未知场景键的错误向上传（矩阵写错键名启动即报，
    /// 开发工具宁可显性失败）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(
        &mut self,
        scene: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        if scene.starts_with("rank") {
            self.dest = Dest::Rank;
            self.rank
                .update(cx, |rank, cx| rank.debug_preview_seed(scene, cx));
        } else if scene.starts_with("storage") {
            self.dest = Dest::Storage;
            self.storage
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("liked") {
            self.dest = Dest::Liked;
            self.liked
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("collections") {
            self.dest = Dest::Collections;
            self.collections
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("history") {
            self.dest = Dest::History;
            self.history
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("reservation") {
            self.dest = Dest::Reservations;
            self.reservations
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("follows") {
            self.dest = Dest::Follows;
            self.follows
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("tasks") {
            self.dest = Dest::Tasks;
            self.tasks
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("settings") {
            self.dest = Dest::Settings;
            self.settings_page
                .update(cx, |view, cx| view.debug_preview_seed(scene, window, cx));
        } else if scene.starts_with("browse") {
            self.dest = Dest::Browse;
            self.browse
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("new-calendar") {
            self.dest = Dest::New;
            self.new_drama
                .update(cx, |view, cx| view.debug_preview_seed("new-calendar", cx));
        } else if scene.starts_with("new") {
            self.dest = Dest::New;
            self.new_drama
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("merge") {
            self.dest = Dest::Merge;
            self.merge
                .update(cx, |view, cx| view.debug_preview_seed(scene, window, cx));
        } else if scene.starts_with("detail") {
            // 详情页场景：确保实体在（不走网络 load，直接灌种子）
            self.dest = Dest::Detail;
            if self.detail.is_none() {
                let v = cx.new(|cx| DetailView::new(self.state.clone(), cx));
                cx.subscribe(&v, |this, _, event: &DetailEvent, cx| {
                    this.on_detail_event(event.clone(), cx);
                })
                .detach();
                self.detail = Some(v);
            }
            if let Some(detail) = &self.detail {
                detail.update(cx, |view, cx| view.debug_preview_seed(scene, cx));
            }
            self.detail_series = Some("7100000".into());
        } else if scene.starts_with("creator") {
            self.dest = Dest::Creator;
            self.creator
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else {
            self.dest = Dest::Home;
            self.feed
                .update(cx, |home, cx| home.debug_preview_seed(scene, window, cx))?;
        }
        cx.notify();
        Ok(())
    }

    // —— 渲染 ——

    /// 窗口关闭一律先问一句（Tauri `CloseRequested` → 前端确认框同款：自绘
    /// ×、Alt+F4、任务栏关闭都走这里；退出会掐断正在跑的下载任务）。
    /// 弹过还来（用户再点一次 ×）直接放行——宁可少问一句，绝不把窗口变成
    /// 关不掉的（Tauri 前端未就绪放行同精神）。
    /// 由 `main.rs` 在壳实体就位后挂上（`Shell::new` 里拿不到自身弱引用）。
    pub(crate) fn install_close_guard(&self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(shell) = this.upgrade() else {
                return true; // 壳都没了：别拦
            };
            if shell.read(cx).exit_asked {
                log::info!("[Shell] 退出确认已弹过，本次关闭直接放行");
                return true;
            }
            // `Entity::update` 不会失败（实体在手）；弹窗就地在本次窗口
            // 回调里开（同一次 update 内，不再跨帧）
            shell.update(cx, |shell, cx| {
                shell.exit_asked = true;
                let view = cx.entity();
                window.open_alert_dialog(cx, move |dialog, _window, _cx| {
                    let view = view.clone();
                    let view_cancel = view.clone();
                    dialog
                        .title("确定要退出吗？")
                        .description("退出会中断正在进行的下载任务，播放进度已实时保存。")
                        .ok_text("退出")
                        .cancel_text("取消")
                        // 组件库的取消钮默认不显示（show_cancel=false）：只
                        // 设文案不给按钮 = 确认框没有取消键（Tauri 确认框
                        // 是「取消 + 确认」两键，Esc 只是额外通道）
                        .show_cancel(true)
                        .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                        .on_ok(move |_, _window, cx| {
                            log::info!("[Shell] 用户在退出确认框里选了退出");
                            cx.quit();
                            true
                        })
                        // 取消/点背景关掉：把闸门放回未问状态，下次关闭
                        // 还会再问（Tauri 取消后对话框收起 = 下次照问）
                        .on_cancel(move |_, _window, cx| {
                            view_cancel.update(cx, |shell, cx| {
                                shell.exit_asked = false;
                                cx.notify();
                            });
                            true
                        })
                });
            });
            false // 拦下这次关闭，等确认框结果
        });
    }

    /// 顶栏（自绘，Tauri `app-shell` header 的 44px 横贯薄行对齐）：
    /// 中部是各页分类 tab 的居中槽（Tauri `TOP_BAR_SLOT_ID` portal 同位），
    /// 右侧置顶 / 外观 / 窗口钮；空白带按住拖窗（>4px 才起拖）、双击最大化。
    ///
    /// 为什么自绘：组件库 `TitleBar` 为 Windows 把整条 bar 标成
    /// `WindowControlArea::Drag`（OS 级 HTCAPTION），落进去的可点控件点击
    /// 全被原生命中测试吞成拖拽（置顶钮上一版就栽在这）。自绘后拖拽改手动
    /// （`start_window_move`），控件走正常命中路径；窗口钮仍用
    /// `WindowControlArea`（Min/Max/Close）交给 OS 原生处理——关闭请求照旧
    /// 走 WM_CLOSE 的退出确认守卫。
    fn render_toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let border = theme.border;
        let bg = theme.sidebar;
        let fg = theme.secondary_foreground;
        let hover_bg = theme.secondary_hover;
        let danger = theme.danger;
        let danger_fg = theme.danger_foreground;
        let mac = cfg!(target_os = "macos");

        // 竖屏不渲染 tab 槽（Tauri 4376496 同款：388px 窄条里整排 tab 与
        // 右侧钮必然叠压；竖屏=沉浸看剧，切 tab 退出竖屏再说）
        let in_portrait = crate::portrait::is_active();
        let tabs = (!in_portrait).then(|| self.render_top_tabs(cx)).flatten();
        let pinned = self.feed.read(cx).pinned();
        let is_maximized = window.is_maximized();

        // 置顶（Tauri 顶栏钉住钮同位）：与小屏紧凑条共用同一份状态
        let pin_button = Button::new("toolbar-pin")
            .icon(if pinned {
                IconName::Pin
            } else {
                IconName::PinOff
            })
            .ghost()
            .small()
            .tooltip(if pinned {
                "取消置顶"
            } else {
                "置顶窗口"
            })
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.feed
                    .update(cx, |view, cx| view.toggle_pinned(window, cx));
                cx.notify();
            }));

        // 外观（Tauri ThemeSwitch 同位）：三档菜单语义与设置页外观卡一致，
        // 点击回调拿不到壳的 Context，经实体句柄回写
        let view = cx.entity();
        let current = self.theme;
        let theme_menu = move |menu: gpui_kit::component::menu::PopupMenu,
                               _window: &mut Window,
                               _cx: &mut Context<gpui_kit::component::menu::PopupMenu>|
              -> gpui_kit::component::menu::PopupMenu {
            let mut menu = menu.label("外观");
            for (value, label) in [
                (crate::ui_prefs::ThemePref::Auto, "跟随系统"),
                (crate::ui_prefs::ThemePref::Light, "亮色"),
                (crate::ui_prefs::ThemePref::Dark, "暗色"),
            ] {
                let view = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(label)
                        .checked(current == value)
                        .on_click(move |_, _window, cx| {
                            view.update(cx, |shell, cx| shell.set_theme_pref(value, cx));
                        }),
                );
            }
            menu
        };
        let theme_icon = match current {
            crate::ui_prefs::ThemePref::Auto => IconName::Monitor,
            crate::ui_prefs::ThemePref::Light => IconName::Sun,
            crate::ui_prefs::ThemePref::Dark => IconName::Moon,
        };
        let theme_button = Button::new("toolbar-theme")
            .icon(theme_icon)
            .ghost()
            .small()
            .tooltip("外观")
            .dropdown_menu(theme_menu);

        // 窗口钮（Windows/Linux；macOS 是原生红绿灯）：Tauri WindowButtons
        // 同款 40px 方块、关闭键 hover 红底；点击交 OS 原生命中处理
        let window_controls = (!mac).then(|| {
            let button = |id: &'static str,
                          icon: IconName,
                          area: gpui_kit::WindowControlArea,
                          is_close: bool| {
                div()
                    .id(id)
                    .flex()
                    .w(gpui_kit::px(40.0))
                    .h_full()
                    .justify_center()
                    .items_center()
                    .text_color(fg)
                    .hover(move |style| {
                        if is_close {
                            style.bg(danger).text_color(danger_fg)
                        } else {
                            style.bg(hover_bg)
                        }
                    })
                    .window_control_area(area)
                    .child(Icon::new(icon).small())
            };
            h_flex()
                .h_full()
                .flex_shrink_0()
                .child(button(
                    "win-min",
                    IconName::WindowMinimize,
                    gpui_kit::WindowControlArea::Min,
                    false,
                ))
                .child(button(
                    "win-max",
                    if is_maximized {
                        IconName::WindowRestore
                    } else {
                        IconName::WindowMaximize
                    },
                    gpui_kit::WindowControlArea::Max,
                    false,
                ))
                .child(button(
                    "win-close",
                    IconName::WindowClose,
                    gpui_kit::WindowControlArea::Close,
                    true,
                ))
        });

        h_flex()
            .id("app-toolbar")
            .relative()
            .h(gpui_kit::px(44.0))
            .flex_shrink_0()
            .items_center()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            // macOS 给原生红绿灯让位（Tauri pl-[76px] 同款）
            .pl(gpui_kit::px(if mac { 76.0 } else { 12.0 }))
            .pr_0()
            // 拖窗/双击（见函数头注释）：阈值防按钮误拖
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _, _| {
                    this.toolbar_drag = Some(ev.position);
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.toolbar_drag = None;
                }),
            )
            .on_mouse_move(
                cx.listener(|this, ev: &gpui_kit::MouseMoveEvent, window, cx| {
                    let Some(start) = this.toolbar_drag else {
                        return;
                    };
                    let dx = (ev.position.x - start.x).as_f32().abs();
                    let dy = (ev.position.y - start.y).as_f32().abs();
                    if dx > 4.0 || dy > 4.0 {
                        this.toolbar_drag = None;
                        #[cfg(target_os = "windows")]
                        {
                            let hwnd = crate::win32::hwnd_of(window);
                            log::info!("[Toolbar] 起拖窗口（hwnd {hwnd:?}，位移 {dx:.0},{dy:.0}）");
                            // WM_NCLBUTTONDOWN 会同步进系统移动循环，必须挪到
                            // 实体更新之外——事件分发当口同步派发 WM_* 会撞
                            // gpui 的借用（隐身藏显 ShowWindow 同款坑，见
                            // `home/incognito.rs` 的 IncognitoStep 模式）。
                            // 延迟一拍只是微秒级，物理按键仍按着，移动循环照进
                            cx.spawn(async move |_this, _cx| {
                                if let Some(hwnd) = hwnd {
                                    crate::win32::start_window_move(hwnd);
                                }
                            })
                            .detach();
                        }
                        #[cfg(not(target_os = "windows"))]
                        window.start_window_move();
                    }
                }),
            )
            .on_double_click(cx.listener(|this, _, window, cx| {
                this.toolbar_drag = None;
                // macOS 要的是系统标题栏双击语义（含「移动并调整大小」等
                // 原生行为）；其余平台切最大化。同样挪出实体更新（见上）
                if cfg!(target_os = "macos") {
                    window.titlebar_double_click();
                    return;
                }
                #[cfg(target_os = "windows")]
                {
                    let hwnd = crate::win32::hwnd_of(window);
                    cx.spawn(async move |_this, _cx| {
                        if let Some(hwnd) = hwnd {
                            crate::win32::toggle_maximize(hwnd);
                        }
                    })
                    .detach();
                }
                #[cfg(not(target_os = "windows"))]
                window.zoom_window();
            }))
            // 中部 tab 槽：绝对定位真居中（Tauri 同款——左右组宽度不等时
            // flex 剩余空间居中有肉眼可见偏移）；容器无命中盒，空白带不挡拖窗
            .children(tabs.map(|tabs| {
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .h_full()
                    .justify_center()
                    .items_center()
                    .child(tabs)
            }))
            .child(
                h_flex()
                    .ml_auto()
                    .h_full()
                    .items_center()
                    .gap_1()
                    .child(pin_button)
                    .child(theme_button)
                    .children(window_controls),
            )
            .into_any_element()
    }

    /// 顶栏中部 tab 槽的内容：当前目的地页面的分类 tab（Tauri 各页
    /// `TopBarTabsPortal` 同位）。没有 tab 的页面返回 None（槽留空）。
    fn render_top_tabs(&mut self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        match self.dest {
            Dest::Home => self.feed.update(cx, |view, cx| view.render_top_tabs(cx)),
            Dest::Rank => self.rank.update(cx, |view, cx| view.render_top_tabs(cx)),
            Dest::New => self
                .new_drama
                .update(cx, |view, cx| view.render_top_tabs(cx)),
            _ => None,
        }
    }

    /// 顶栏外观菜单的落点：与设置页同一段实现（应用 + 落盘 + 通知观察者）。
    fn set_theme_pref(&mut self, pref: crate::ui_prefs::ThemePref, cx: &mut Context<Self>) {
        self.settings_page
            .update(cx, |view, cx| view.set_theme(pref, cx));
    }

    /// 侧栏（Tauri app-sidebar 对齐）：13 目的地，展开 224px 图标+文字、
    /// 折叠 56px 纯图标（tooltip 出标签）；追更带未读角标；贴底账户区
    /// （头像/昵称；已登录点进设置页）+ 折叠钮。
    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let collapsed = self.sidebar_collapsed;
        // 头像懒加载守卫：账号在且 URL 变化才触发一次（幂等）
        self.ensure_account_avatar(cx);
        let theme = cx.theme();
        let account = self.state.settings().account;
        let logged_in = account.is_some();
        let unread = self.follow_unread;
        let avatar = self.account_avatar.clone();
        let account_label = account
            .as_ref()
            .map(|a| {
                if a.user_name.is_empty() {
                    "我的".to_string()
                } else {
                    a.user_name.clone()
                }
            })
            .unwrap_or_else(|| "我的".to_string());
        let item = |dest: Dest| {
            let active = self.dest == dest;
            let badge = (dest == Dest::Follows && unread > 0).then_some(unread);
            let badge_text = |n: usize| {
                if n > 99 {
                    "99+".to_string()
                } else {
                    n.to_string()
                }
            };
            let row = div()
                .id(ElementId::from(format!("nav-{}", dest.label())))
                .flex()
                .items_center()
                .gap_3()
                .rounded_md()
                .px_3()
                .py(gpui_kit::px(7.0))
                .text_sm()
                .cursor_pointer()
                .when(collapsed, |s| s.justify_center().px_0())
                .text_color(if active {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .when(active, |s| s.bg(theme.secondary))
                .hover(|s| s.bg(theme.secondary_hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.dest == dest {
                        return;
                    }
                    this.enter_dest(dest, cx);
                }))
                .child(
                    div()
                        .relative()
                        .flex_shrink_0()
                        .child(
                            gpui_kit::component::Icon::new(dest.icon())
                                .small()
                                .text_color(if active {
                                    theme.foreground
                                } else {
                                    theme.muted_foreground
                                }),
                        )
                        // 折叠态：小角标压在图标右上（Tauri 同款）
                        .children((collapsed && badge.is_some()).then(|| {
                            div()
                                .absolute()
                                .top(gpui_kit::px(-6.0))
                                .right(gpui_kit::px(-8.0))
                                .rounded_full()
                                .bg(theme.danger)
                                .px_1()
                                .text_size(gpui_kit::px(9.0))
                                .text_color(theme.danger_foreground)
                                .child(SharedString::from(badge_text(badge.unwrap_or(0))))
                        })),
                )
                .when(!collapsed, |s| s.child(dest.label()))
                // 展开态：数字胶囊贴行尾（Tauri 同款）
                .children((!collapsed && badge.is_some()).then(|| {
                    div()
                        .ml_auto()
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(theme.danger)
                        .px_1p5()
                        .py_0p5()
                        .text_size(gpui_kit::px(10.0))
                        .text_color(theme.danger_foreground)
                        .child(SharedString::from(badge_text(badge.unwrap_or(0))))
                }));
            if collapsed {
                row.tooltip(move |window, cx| Tooltip::new(dest.label()).build(window, cx))
            } else {
                row
            }
        };
        v_flex()
            .w(gpui_kit::px(if collapsed { 56.0 } else { 224.0 }))
            .h_full()
            .flex_shrink_0()
            .bg(theme.background)
            .border_r_1()
            .border_color(theme.border)
            .p_2()
            .gap_1()
            .id("shell-sidebar")
            .overflow_y_scroll()
            .children(Dest::ALL.map(item))
            // 贴底账户区（Tauri AccountButton 同位）：未登录开登录面板；
            // 已登录点进设置页（退出登录在设置页账户卡，Tauri 同款）
            .child(
                div()
                    .border_t_1()
                    .border_color(theme.border)
                    .pt_2()
                    .mt_auto()
                    .child(
                        div()
                            .id("nav-account")
                            .flex()
                            .items_center()
                            .gap_3()
                            .rounded_md()
                            .px_3()
                            .py(gpui_kit::px(7.0))
                            .text_sm()
                            .cursor_pointer()
                            .text_color(theme.muted_foreground)
                            .hover(|s| s.bg(theme.secondary_hover))
                            .when(collapsed, |s| s.justify_center().px_0())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if logged_in {
                                    this.enter_dest(Dest::Settings, cx);
                                } else {
                                    // 登录面板长在首页舞台：先切回首页再开
                                    this.dest = Dest::Home;
                                    this.feed.update(cx, |home, cx| {
                                        home.resume_if_idle(cx);
                                        home.toggle_login_panel(cx);
                                    });
                                    cx.notify();
                                }
                            }))
                            .child(match avatar {
                                Some(image) => img(ImageSource::Render(image))
                                    .size_5()
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .object_fit(ObjectFit::Cover)
                                    .into_any_element(),
                                None => gpui_kit::component::Icon::new(IconName::UserRound)
                                    .small()
                                    .text_color(theme.muted_foreground)
                                    .into_any_element(),
                            })
                            .when(!collapsed, |s| {
                                s.child(account_label.clone())
                                    .children((!logged_in).then(|| {
                                        div()
                                            .ml_auto()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(theme.primary)
                                            .child("登录")
                                    }))
                            }),
                    ),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(theme.border)
                    .pt_2()
                    .child(if collapsed {
                        Button::new("sidebar-expand")
                            .icon(IconName::PanelLeft)
                            .ghost()
                            .small()
                            .tooltip("展开")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_collapsed = false;
                                this.persist_sidebar_collapsed();
                                cx.notify();
                            }))
                            .into_any_element()
                    } else {
                        Button::new("sidebar-collapse")
                            .icon(IconName::PanelLeftClose)
                            .ghost()
                            .small()
                            .label("收起")
                            .tooltip("收起")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_collapsed = true;
                                this.persist_sidebar_collapsed();
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn render_content(&self, _cx: &Context<Self>) -> gpui_kit::AnyElement {
        match self.dest.view() {
            View::Home => self.feed.clone().into_any_element(),
            View::Rank => self.rank.clone().into_any_element(),
            View::Browse => self.browse.clone().into_any_element(),
            View::New => self.new_drama.clone().into_any_element(),
            View::Liked => self.liked.clone().into_any_element(),
            View::Collections => self.collections.clone().into_any_element(),
            View::Follows => self.follows.clone().into_any_element(),
            View::History => self.history.clone().into_any_element(),
            View::Reservations => self.reservations.clone().into_any_element(),
            View::Tasks => self.tasks.clone().into_any_element(),
            View::Settings => self.settings_page.clone().into_any_element(),
            View::Storage => self.storage.clone().into_any_element(),
            View::Merge => self.merge.clone().into_any_element(),
            View::Creator => self.creator.clone().into_any_element(),
            View::Detail => self
                .detail
                .as_ref()
                .expect("Detail 目的地必有实体")
                .clone()
                .into_any_element(),
        }
    }
}

impl gpui_kit::Render for Shell {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // chrome 全藏的两种形态：小屏播放（480×270 纯播放器，Tauri 小屏
        // 同款，拖窗走播放页拖拽条）与播放器沉浸全屏（播放器全屏按钮/F
        // 进入；顶栏最大化钮是另一档「应用全屏」，chrome 保留，对齐 Tauri）
        let in_mini = crate::mini::is_active();
        // 竖屏：侧栏藏掉、顶栏保留（Tauri app-shell 的 `!miniScreen &&
        // !playerFullscreen && !portrait` 门控同款——竖条只放播放器）
        let in_portrait = crate::portrait::is_active();
        let immersive = self.feed.read(cx).immersive_fullscreen();
        let chrome_hidden = in_mini || immersive;
        // 先建好所有吃 cx 的闭包/子渲染，再取 theme（theme 借 cx，两者不能
        // 交叠——闭包里要 &mut Context）
        let toolbar = (!chrome_hidden).then(|| self.render_toolbar(window, cx));
        let sidebar = (!chrome_hidden && !in_portrait).then(|| self.render_sidebar(cx));
        let content = self.render_content(cx);
        let theme = cx.theme();
        v_flex()
            .id("shell")
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .children(toolbar)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    // 小屏/沉浸全屏时侧栏藏掉（视频区吃满，播放跟随窗口）
                    .children(sidebar)
                    .child(
                        // 内容区：吃掉剩余宽度；过载时只允许它收缩（coding-guides
                        // 「flex_1 仍可能拒绝收缩」），滚动归视图自己。
                        v_flex().flex_1().min_w_0().child(content),
                    ),
            )
    }
}
