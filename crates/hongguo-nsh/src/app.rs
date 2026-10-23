//! 根组件：应用外壳（侧栏 + 顶栏 + 内容区），与 Tauri 壳同构。
//!
//! 版面对齐 Tauri 壳：左侧栏顶部是应用名 + 四个页签，顶栏是当前页标题 +
//! 右侧状态与小屏开关，内容区无边距铺满。首页内容是**沉浸式播放**
//! （`home.rs`）：视频铺满整个内容区，悬浮层按 hgplayer/Tauri 的位置叠在画面上。
//!
//! 组件体只跑一次（nsh 的组件语义），状态全是 `Copy` 信号句柄，闭包随便捕获。
//!
//! - **推荐流 + 播放**（`home.rs` / `home_ui.rs`）：真实接口数据、点卡片不进
//!   列表页，直接在同一个 `video` 上换源；
//! - **导航**：侧栏切页（栈重置），返回键弹栈；
//! - **事件泵**：`ServiceEvent` 总线接进 UI（`pump.rs`）；
//! - **窗口形态**：小窗进出（`platform_ext`，几何在平台层）。
//!
//! 排行榜 / 找剧 / 设置仍是占位面板，按 `docs/nsh-shell-plan.md` §5 分期迁。

use nsh_core::ui::prelude::*;

use crate::account;
use crate::i18n::t;
use crate::nav::{FeedNav, Frameless, Incognito, MiniMode, Nav, Page};
use crate::pages;
use crate::platform;
use crate::play_req;
use crate::player;
use crate::pump;

/// 侧栏宽度（Tauri `w-56`）。
const SIDEBAR_W: f32 = 224.0;
/// 侧栏行高。
const SIDEBAR_ROW_H: f32 = 36.0;
/// 顶栏高度（Tauri `h-11`）。
const HEADER_H: f32 = 44.0;
/// 顶栏小窗开关的宽度（冒烟脚本按「距右 16、宽 88」取坐标）。
const MINI_TOGGLE_W: f32 = 88.0;

/// The application's root component.
#[component]
pub fn App() -> impl IntoElement {
    let nav = Nav::new();
    let feed = FeedNav::new();
    let mini = MiniMode::new();
    let frameless = Frameless::new();
    let incognito = Incognito::new();
    // 句柄走 context：页面组件不重复声明 props（nsh 的 props 是给 JS
    // 用的解码结构，纯 Rust 壳里传 `Copy` 句柄最省事）。
    provide_context(nav);
    provide_context(feed);
    provide_context(mini);
    provide_context(frameless);
    provide_context(incognito);
    // 事件泵随组件树启动一次（任务跟着 owner 一起销毁）。
    pump::start();
    let events = pump::events();
    // 账号域与跨页播放请求的壳级信号节点先建起来（首个使用点可能在更深的
    // 组件里，那里不再是组件体）。
    account::init();
    play_req::init();

    // 窗口几何恢复（常态槽位，跨启动记忆）：窗口此刻已由 run_app 创建，
    // 按记忆校验落位；没有记忆保持 AppOptions 的默认尺寸。
    platform::restore_normal();

    // 单实例前置请求轮询：第二个实例置位事件后把窗口带到前台。100ms 一拍，
    // 与 Tauri 壳的窗口前置节奏一致。
    spawn(async move {
        loop {
            timer(std::time::Duration::from_millis(100)).await;
            if platform::take_focus_request() {
                platform::bring_to_front();
            }
        }
    });

    // 返回键（桌面端是鼠标侧键）：弹栈。
    on_back_press(move || {
        nav.pop();
        true
    });

    // 无边框播放（hgplayer bare 同款）：chrome 全藏（侧栏+顶栏），整窗
    // 只剩播放器画面；Esc 退出（播放域的 Esc 链）。
    let frameless_on = frameless;
    // 顶栏横贯全宽（Tauri AppShell 同构：logo/应用名在左、分类 tab 居中、
    // 状态与小屏在右），侧栏在顶栏下方而不是长在顶栏里。
    view()
        .w_full()
        .h_full()
        .flex_col()
        .bg(theme().background)
        .child(If::new(
            move || !mini.get() && !frameless_on.get(),
            move || header(nav, feed, events, mini),
        ))
        .child(
            view()
                .flex_1()
                .flex_row()
                .child(If::new(
                    move || !mini.get() && !frameless_on.get(),
                    move || sidebar(nav),
                ))
                .child(content(nav)),
        )
}

/// 左侧栏：四个页签（应用名在顶栏，侧栏只留导航）。
fn sidebar(nav: Nav) -> impl IntoElement {
    let rows: Vec<_> = Page::ALL
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let page = *page;
            let active = move || nav.current() == page;
            view()
                .w_full()
                .h(px(SIDEBAR_ROW_H))
                .flex_row()
                .items_center()
                .px(px(12.0))
                .rounded(px(6.0))
                .bg(move || {
                    if active() {
                        theme().accent
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .child(
                    text(move || t(page.title_key()))
                        .text_size(px(14.0))
                        .text_color(move || {
                            if active() {
                                theme().foreground
                            } else {
                                theme().muted_foreground
                            }
                        }),
                )
                .key(format!("nav-row-{index}"))
                .on_press(move |_: &()| nav.goto(page))
        })
        .collect();

    view()
        .w(px(SIDEBAR_W))
        .h_full()
        .flex_col()
        .gap(px(2.0))
        .px(px(8.0))
        .pt(px(8.0))
        .pb(px(8.0))
        .bg(theme().group_box)
        .border_r_1()
        .border_color(theme().border)
        .children(rows)
}

/// 顶栏（横贯全宽，Tauri AppShell 同构）：logo/应用名在左，分类 tab 居中
/// （绝对定位真居中——左右组宽度不等，flex 剩余空间居中会偏右），运行
/// 状态与小屏开关钉在右端。
fn header(
    nav: Nav,
    feed: FeedNav,
    events: Signal<pump::ShellEvents>,
    mini: MiniMode,
) -> impl IntoElement {
    view()
        .w_full()
        .h(px(HEADER_H))
        .relative()
        .flex_row()
        .items_center()
        .px(px(16.0))
        .gap(px(12.0))
        .bg(theme().group_box)
        .border_b_1()
        .border_color(theme().border)
        // 左：应用名（flex_1 把右组推到行尾；中组是绝对定位居中，不占流）。
        .child(
            text(move || t("app.name"))
                .flex_1()
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme().foreground),
        )
        // 中：分类 tab 居中（首页；其他页顶栏中部留空，导航位置由侧栏
        // 高亮表达——Tauri AppShell 同款口径）。
        .child(If::new(
            move || nav.current() == Page::Home,
            move || {
                view()
                    .absolute()
                    .left(px(0.0))
                    .right(px(0.0))
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .children(
                        player::TABS
                            .iter()
                            .map(|&(id, key)| tab_pill(feed, id, t(key)))
                            .collect::<Vec<_>>(),
                    )
            },
        ))
        // 右：运行状态 + 小屏开关。
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(
                    text(move || {
                        let events = events.get();
                        match events.online_percent {
                            Some(percent) => format!("{} {percent:.0}%", t("common.loading")),
                            None if events.follow_notices > 0 => {
                                format!("{} {}", t("nav.follows.title"), events.follow_notices)
                            }
                            None => String::new(),
                        }
                    })
                    .text_size(px(12.0))
                    .text_color(theme().muted_foreground)
                    .tabular_nums(),
                )
                .child(
                    view()
                        .w(px(MINI_TOGGLE_W))
                        .h(px(28.0))
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.0))
                        .bg(theme().muted)
                        .child(
                            text(move || t("player.miniWindow"))
                                .text_size(px(12.0))
                                .text_color(theme().foreground),
                        )
                        .key("mini-toggle")
                        .on_press(move |_: &()| mini.set(true)),
                ),
        )
}

/// 一个分类 tab 胶囊（选中 = 主色胶囊，与应用其他 tab 同一套语言）。
fn tab_pill(feed: FeedNav, id: &'static str, label: String) -> impl IntoElement {
    let current = feed.current();
    let active = move || current.with(|now| now == id);
    text(move || label.clone())
        .text_size(px(13.0))
        .text_color(move || {
            if active() {
                theme().background
            } else {
                theme().muted_foreground
            }
        })
        .px(px(12.0))
        .py(px(4.0))
        .rounded(px(999.0))
        .bg(move || {
            if active() {
                theme().primary
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .key(format!("tab-{id}"))
        .on_press(move |_: &()| feed.pick(id))
}

/// 内容区：按栈顶渲染页面（首页是沉浸式播放，铺满无边距；历史/收藏/追更/
/// 设置是账号域整页，其余占位待 P3 迁入）。登录面板（模态）盖在内容区上，
/// 覆盖范围与 GPUI 壳的登录面板（盖在首页舞台）一致。
fn content(nav: Nav) -> impl IntoElement {
    view()
        .flex_1()
        .w_full()
        .h_full()
        .relative()
        .overflow_hidden()
        .child(If::new(
            move || nav.current() == Page::Home,
            move || player::PlayerPage::element(()),
        ))
        .child(If::new(
            move || nav.current() == Page::Rank,
            move || placeholder("rank.empty"),
        ))
        .child(If::new(
            move || nav.current() == Page::Browse,
            move || placeholder("search.empty"),
        ))
        .child(If::new(
            move || nav.current() == Page::History,
            move || pages::history::HistoryPage::element(()),
        ))
        .child(If::new(
            move || nav.current() == Page::Collections,
            move || pages::collections::CollectionsPage::element(()),
        ))
        .child(If::new(
            move || nav.current() == Page::Follows,
            move || pages::follows::FollowsPage::element(()),
        ))
        .child(If::new(
            move || nav.current() == Page::Reservations,
            move || pages::reservations::ReservationsPage::element(()),
        ))
        .child(If::new(
            move || nav.current() == Page::Tasks,
            move || pages::tasks::TasksPage::element(()),
        ))
        .child(If::new(
            move || nav.current() == Page::Settings,
            move || pages::settings::SettingsPage::element(()),
        ))
        // 登录面板（模态，账号域各入口共用；开合由 account::OPEN 驱动）。
        .child(account::login_overlay())
}

/// 占位面板：用一个既有 i18n 词条说明「这一页还没迁进来」。
fn placeholder(key: &'static str) -> impl IntoElement {
    view()
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .child(
            text(move || t(key))
                .text_size(px(14.0))
                .text_color(theme().muted_foreground),
        )
}
