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

mod account;
#[cfg(feature = "screenshot")]
mod debug;
#[cfg(feature = "screenshot")]
mod debug_preview;
mod events;
mod nav;
mod render;
mod sidebar;
mod topbar;
mod topbar_menu;
mod window;

use std::sync::Arc;

use gpui_kit::Entity;
use gpui_kit::prelude::*;
use gpui_kit::{RenderImage, Window};
use hongguo_service::app_state::AppStateInner;

use crate::detail::DetailView;
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

pub(crate) use self::nav::Dest;

/// 窗口几何防抖落盘间隔（Tauri `window_state::spawn_saver` 的 500ms 同参）。
/// 移动/缩放每停一下就在这之后落盘，退出路径（关闭回调 / Cmd+Q / 强杀）
/// 不再决定记忆能否保住。
const GEOMETRY_SAVE_DEBOUNCE_MS: u64 = 500;

/// 壳窗口实体：持有各视图实体，切换当前目的地。服务面由视图各自持有。
pub struct Shell {
    dest: Dest,
    state: Arc<AppStateInner>,
    sidebar_collapsed: bool,
    /// 主题偏好（auto 时随系统外观重算；设置页改动即存即生效）。
    theme: crate::ui_prefs::ThemePref,
    /// 当前界面语言。
    locale: crate::i18n::Locale,
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
    /// 封面解码结果的内存缓存（gpui 的 `ImageCache` 实现，有张数上限）：
    /// 挂在壳根节点上，全应用的封面图都走它。
    cover_image_cache: Entity<crate::cover_image_cache::CoverImageCache>,
    /// 详情页当前加载的剧（同剧重复点击不重载）。
    detail_series: Option<String>,
    /// 侧栏追更未读角标（本地通知未读数；未登录恒 0）。
    follow_unread: usize,
    /// 退出确认框已经弹过（Tauri CloseRequested 的「已问过」闸门）：再收到
    /// 关闭请求就直接放行——宁可少问一句，绝不把窗口变成关不掉的
    ///（Tauri「前端未就绪就放行」同精神）。
    exit_asked: bool,
    /// 上次采样的常态窗口几何 [x, y, w, h]（render 逐帧刷新，非小屏/
    /// 竖屏/全屏/最大化/最小化才覆盖）。随关闭落盘，下次启动恢复（issue
    /// #11）。
    win_bounds: [f32; 4],
    /// 上次采样的小屏几何 [x, y, w, h]（小屏中逐帧刷新）。三种窗态各记
    /// 各的槽位，切形态互不覆盖；进小屏时由 [`crate::mini`] 取用。
    mini_bounds: [f32; 4],
    /// 上次采样的竖屏几何 [x, y, w, h]（竖屏中逐帧刷新；进竖屏时由
    /// [`crate::portrait`] 取用）。
    portrait_bounds: [f32; 4],
    /// 采样到的几何还没落盘（500ms 防抖 saver 用；见
    /// [`GEOMETRY_SAVE_DEBOUNCE_MS`]）。
    bounds_dirty: bool,
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
        // 系统播放器：窗口透明（GPUI Metal 层 clear alpha=0）+ 背景铺黑，
        // 视频 NSView / DComp 视频位插在 GPUIView 之下。
        crate::platform::prepare_system_window(window);
        let feed = cx.new(|cx| HomeView::new(state.clone(), window, cx));
        // 无人值守启动（终端后台拉起）时窗口可能不是 key window，GPUI 就
        // 不再请求帧；系统播放器要在首帧 render 时挂 NSView，这一步不能省。
        {
            let wh = window.window_handle();
            cx.spawn(async move |_this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(800))
                    .await;
                let active = wh
                    .update(cx, |_, window, _| {
                        let active = window.is_window_active();
                        window.activate_window();
                        active
                    })
                    .unwrap_or(false);
                log::info!("[Shell] 启动激活：之前 active={active}");
            })
            .detach();
        }
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
            // 下载提交成功：切下载任务页（Tauri navigate('/tasks') 同款）
            HomeEvent::OpenTasks => this.enter_dest(Dest::Tasks, cx),
            // chrome 显隐变了（无边框/沉浸全屏）：壳重绘才会重读
            // `immersive_fullscreen()`，否则顶栏/侧栏停在旧状态
            HomeEvent::ChromeChanged => cx.notify(),
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
            locale: ui_prefs.locale,
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
            cover_image_cache: crate::cover_image_cache::CoverImageCache::new(cx),
            detail: None,
            detail_series: None,
            follow_unread: 0,
            exit_asked: false,
            win_bounds: [0.0; 4],
            mini_bounds: [0.0; 4],
            portrait_bounds: [0.0; 4],
            bounds_dirty: false,
            account_avatar: None,
            account_avatar_url: None,
            account_avatar_pending: false,
            toolbar_drag: None,
        };
        // 启动目的地不是首页：按住首页信息流的冷启动自动开播——信息流
        // 异步加载完成时人还在别的页，不按住的话 after_feed_loaded 会照常
        // 起播（人不在首页，剧在播）。回首页走 resume_if_idle 续播。
        if dest != Dest::Home {
            shell.feed.update(cx, |home, _cx| home.hold_cold_start());
        }
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
        // 壳级轮询：全局快捷键（老板键）与「第二次启动前置窗口」。
        // 两项能力都由平台层提供（`SHELL_POLLER`）——macOS 尚未实现注册与
        // 单实例前置，不空转一条每秒 10 拍的循环。
        //
        // WM_HOTKEY 只能在轮询里递进 GPUI（窗口过程跑在主线程消息循环里，
        // 拿不到 GPUI 上下文），窗口过程是「按下沿置位」，100ms 一拍不会漏。
        if crate::platform::SHELL_POLLER {
            let handle = window.window_handle();
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(100))
                        .await;
                    // 第二次启动请求前置窗口（单实例；Tauri bring_back_active_window
                    // 同款：显示 + 还原最小化 + 聚焦）
                    if crate::platform::take_focus_request() {
                        let shown = handle
                            .update(cx, |_, window, _| {
                                crate::platform::restore_from_minimized(window);
                                crate::platform::show_foreground(window);
                                crate::platform::visibility(window)
                            })
                            .unwrap_or(false);
                        log::info!("[Shell] 第二次启动请求：窗口已带到前台（可见={shown}）");
                    }
                    if !crate::boss_key::take_fired() {
                        continue;
                    }
                    // 先查可见性：藏显放实体更新之外——它同步派发窗口消息，
                    // gpui 的窗口过程要更新窗口状态，在 update 里调用会撞
                    // 借用（实测日志刷「RefCell already borrowed」六连）。
                    let Ok(visible) =
                        handle.update(cx, |_, window, _| crate::platform::visibility(window))
                    else {
                        return; // 窗口已关
                    };
                    let ok = this
                        .update(cx, |shell, cx| {
                            // 入参是「这次要藏起来吗」：当前可见 = 本次按下去
                            // 是藏（暂停），当前不可见 = 本次是按回来（续播）。
                            // 这里写反过一次（传 !visible），实测症状是「藏起来
                            // 还在响、按回来反而暂停」。
                            shell.boss_key_playback(visible, cx);
                        })
                        .is_ok();
                    if !ok {
                        return;
                    }
                    let _ = handle.update(cx, |_, window, _| {
                        if visible {
                            crate::platform::hide(window);
                        } else {
                            crate::platform::show_foreground(window);
                        }
                    });
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
        // 预览不挂：预览场景的主题由矩阵按 case 指定（scene 的 mode），
        // headless 窗口一挂上这个回调就会用宿主外观（Light）把场景主题
        // 盖掉——dark 场景全部渲染成亮色（2026-10-18 实测）。
        if !crate::preview::is_active() {
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.theme == crate::ui_prefs::ThemePref::Auto {
                    let mode = this.theme.resolve(window.appearance());
                    crate::tokens::apply(mode, cx);
                }
            })
            .detach();
        }
        shell
    }
}
