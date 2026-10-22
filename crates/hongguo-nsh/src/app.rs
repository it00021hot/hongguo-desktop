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

use crate::i18n::t;
use crate::nav::{FeedNav, MiniMode, Nav, Page};
use crate::platform_ext;
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
    // 三个句柄走 context：页面组件不重复声明 props（nsh 的 props 是给 JS
    // 用的解码结构，纯 Rust 壳里传 `Copy` 句柄最省事）。
    provide_context(nav);
    provide_context(feed);
    provide_context(mini);
    // 事件泵随组件树启动一次（任务跟着 owner 一起销毁）。
    pump::start();
    let events = pump::events();

    // 单实例前置请求轮询：第二个实例置位事件后把窗口带到前台。100ms 一拍，
    // 与 Tauri 壳的窗口前置节奏一致。
    spawn(async move {
        loop {
            timer(std::time::Duration::from_millis(100)).await;
            if platform_ext::take_focus_request() {
                platform_ext::bring_to_front();
            }
        }
    });

    // 返回键（桌面端是鼠标侧键）：弹栈。
    on_back_press(move || {
        nav.pop();
        true
    });

    view()
        .w_full()
        .h_full()
        .flex_row()
        .bg(theme().background)
        // 小屏播放：chrome 全藏，整窗只剩播放器（Tauri `miniScreen` 同款）。
        .child(If::new(move || !mini.get(), move || sidebar(nav)))
        .child(
            view()
                .flex_1()
                .h_full()
                .flex_col()
                .child(If::new(
                    move || !mini.get(),
                    move || header(nav, feed, events, mini),
                ))
                .child(content(nav)),
        )
}

/// 左侧栏：应用名 + 四个页签（顶部不留内边距，与顶栏标题同一基线）。
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
        .pb(px(8.0))
        .bg(theme().group_box)
        .border_r_1()
        .border_color(theme().border)
        // 标题行与顶栏同高、顶部不留内边距：两侧文字落在同一条水平线上。
        .child(
            view()
                .w_full()
                .h(px(HEADER_H))
                .flex_row()
                .items_center()
                .px(px(4.0))
                .child(
                    text(move || t("app.name"))
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme().foreground),
                ),
        )
        .children(rows)
}

/// 顶栏：当前页标题（首页换成分类 tab）+ 运行状态 + 小屏开关（Tauri 的薄行形态）。
///
/// 分类 tab 放在顶栏中部而不是画面上：Tauri 壳的首页 tab 就是 portal 进顶栏
/// 插槽的（`top-bar-tabs.tsx`），画面顶部不再有第二层 tab 条。
fn header(
    nav: Nav,
    feed: FeedNav,
    events: Signal<pump::ShellEvents>,
    mini: MiniMode,
) -> impl IntoElement {
    view()
        .w_full()
        .h(px(HEADER_H))
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .border_b_1()
        .border_color(theme().border)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                // 首页：分类 tab 胶囊；其他页：页标题。
                .child(If::new(
                    move || nav.current() == Page::Home,
                    move || {
                        view().flex_row().items_center().gap(px(4.0)).children(
                            player::TABS
                                .iter()
                                .map(|&(id, key)| tab_pill(feed, id, t(key)))
                                .collect::<Vec<_>>(),
                        )
                    },
                ))
                .child(If::new(
                    move || nav.current() != Page::Home,
                    move || {
                        text(move || t(nav.current().title_key()))
                            .text_size(px(16.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme().foreground)
                    },
                )),
        )
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

/// 内容区：按栈顶渲染页面（首页是沉浸式播放，铺满无边距）。
fn content(nav: Nav) -> impl IntoElement {
    view()
        .flex_1()
        .w_full()
        .h_full()
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
            move || nav.current() == Page::Settings,
            move || placeholder("settings.appearance"),
        ))
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
