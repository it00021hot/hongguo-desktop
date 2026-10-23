//! 沉浸流信息层与播放反馈：@剧名/热度/角标/简介压画面左下，连播指示与
//! 「观看完整短剧」横条、暂停快捷键提示、顶部加载细条、右上错误条、
//! 中央加载面板。位置与层级对齐 Tauri `immersive-info-overlay.tsx` /
//! `loadbar.tsx` / `error-bar.tsx` / `loading-panel.tsx`。

use nsh_core::ui::prelude::*;

use crate::i18n::t;

use super::icons;

/// 控制栏总高（左下文案与横条的定位基准；控制栏 pt16 + scrub16 +
/// 按钮32 + pb10 + gap8 ≈ 100，取整留 4px 呼吸）。
pub(crate) const BOTTOM_BAR_H: f32 = 104.0;

/// 沉浸信息层状态束（悬浮层显隐在 `Stage::chrome_shown`，由调用方传入）。
#[derive(Clone, Copy)]
pub(crate) struct InfoState {
    /// 剧名（@标题；档案未到为空）。
    pub title: Signal<String>,
    /// 集号（从 1 起；0 = 未就绪不显示）。
    pub vid_index: Signal<u32>,
    /// 全剧总集数（0 = 未知）。
    pub episode_count: Signal<u32>,
    /// 简介（video_detail；空 = 未到/没有）。
    pub intro: Signal<String>,
    /// 简介展开态。
    pub intro_expanded: Signal<bool>,
    /// 信息流条目自带的热度文本（「1705万」形态，剧名上方琥珀色）。
    pub heat_text: Signal<String>,
    /// 季角标（「第1季」）与运营角标（「新剧/爆剧」）。
    pub season_tag: Signal<String>,
    pub badge: Signal<String>,
}

impl InfoState {
    pub(crate) fn new() -> Self {
        Self {
            title: signal(String::new()),
            vid_index: signal(0),
            episode_count: signal(0),
            intro: signal(String::new()),
            intro_expanded: signal(false),
            heat_text: signal(String::new()),
            season_tag: signal(String::new()),
            badge: signal(String::new()),
        }
    }
}

/// 左下信息块：热度行 / 角标+@剧名 / 集数行 / 简介（可展开）。
/// 控制栏弹出时整体抬到其上沿，收起时落回低位并一起淡出
/// （Tauri 的 ChromeSurface 同一款显隐）。
pub(crate) fn info_overlay(s: InfoState, chrome_shown: Signal<bool>) -> impl IntoElement {
    view()
        .absolute()
        .left(px(12.0))
        .max_w(px(620.0))
        .flex_col()
        .gap(px(4.0))
        // 容器自身穿透（空白区不挡舞台的播放/暂停与滑动换剧）；「展开」
        // 等子按钮照常可点。
        .pointer_events(PointerEvents::BoxNone)
        .bottom(move || {
            if chrome_shown.get() {
                px(BOTTOM_BAR_H + 8.0)
            } else {
                px(56.0)
            }
        })
        .opacity(move || if chrome_shown.get() { 1.0 } else { 0.0_f32 })
        .child(If::new(
            move || !s.heat_text.get().is_empty(),
            move || {
                text(move || s.heat_text.get())
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgba(0xfcd34de6))
            },
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(If::new(
                    move || !s.badge.get().is_empty(),
                    move || pill(move || s.badge.get(), rgba(0xef4444e6), rgb(0xffffff)),
                ))
                .child(If::new(
                    move || !s.season_tag.get().is_empty(),
                    move || pill(move || s.season_tag.get(), rgba(0xffffff33), rgb(0xffffff)),
                ))
                .child(
                    text(move || format!("@{}", s.title.get()))
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0xffffff))
                        .number_of_lines(1),
                ),
        )
        .child(
            text(move || {
                let index = s.vid_index.get();
                // 集号未定位（分集表未到）不渲染「第0集」。
                let count = s.episode_count.get();
                let mut line = if index > 0 {
                    t("player.epShort").replace("{index}", &index.to_string())
                } else {
                    String::new()
                };
                if count > 0 {
                    let total = t("player.totalEpisodes").replace("{count}", &count.to_string());
                    if line.is_empty() {
                        line = total;
                    } else {
                        line.push_str(&format!(" · {total}"));
                    }
                }
                line
            })
            .text_size(px(12.0))
            .text_color(rgba(0xffffffd9))
            .tabular_nums(),
        )
        // 简介块最多占舞台约三分之一宽，两行收起、可展开（hgplayer 同款
        // 量级）。number_of_lines 不吃响应式闭包，收起/展开是两个分支；
        // 宽度约束直接给到 text（容器的 max_w 管不住文本测量宽）。
        .child(If::new(
            move || !s.intro.get().is_empty(),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(If::new(
                        move || !s.intro_expanded.get(),
                        move || {
                            text(move || s.intro.get())
                                .text_size(px(12.0))
                                .text_color(rgba(0xffffffb3))
                                .max_w(px(400.0))
                                .number_of_lines(2)
                        },
                    ))
                    .child(If::new(
                        move || s.intro_expanded.get(),
                        move || {
                            text(move || s.intro.get())
                                .text_size(px(12.0))
                                .text_color(rgba(0xffffffb3))
                                .max_w(px(400.0))
                        },
                    ))
                    .child(If::new(
                        move || s.intro.get().chars().count() > 40,
                        move || {
                            text(move || {
                                if s.intro_expanded.get() {
                                    t("player.introCollapse")
                                } else {
                                    t("player.introExpand")
                                }
                            })
                            .text_size(px(12.0))
                            .text_color(rgba(0xffffff99))
                            .on_press(move |_: &()| s.intro_expanded.update(|v| *v = !*v))
                        },
                    ))
            },
        ))
}

/// 小标签胶囊（角标）。
fn pill(label: impl Fn() -> String + 'static, bg: Color, fg: Color) -> impl IntoElement {
    view()
        .px(px(4.0))
        .h(px(16.0))
        .items_center()
        .rounded(px(3.0))
        .bg(bg)
        .child(
            text(label)
                .text_size(px(10.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg),
        )
}

/// 连播指示（左上角「‹ 第N集」，点返回解除连播；手机端同款形态）。
pub(crate) fn binge_indicator(
    chrome_shown: Signal<bool>,
    vid_index: Signal<u32>,
    exit: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .absolute()
        .top(px(12.0))
        .left(px(12.0))
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .h(px(24.0))
        .px(px(8.0))
        .pl(px(4.0))
        .rounded_full()
        .bg(rgba(0x0000004d))
        .child(icons::chevron_left(14.0, || rgb(0xffffff)))
        .child(
            text(move || t("player.episodeNo").replace("{index}", &vid_index.get().to_string()))
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(0xffffff)),
        )
        .on_press(move |_: &()| exit())
        // 显隐跟 chrome 一票（不另立时机），简化：常显由外层 If 决定，
        // chrome 收起时降透明度而不是移除（少一次子树重建）。
        .opacity(move || if chrome_shown.get() { 1.0 } else { 0.0_f32 })
}

/// 「观看完整短剧 · 全N集」横条：进入选中剧连播态的唯一手动入口
/// （选中后不渲染——它的使命就是进入选中态）。
pub(crate) fn binge_entry(
    chrome_shown: Signal<bool>,
    episode_count: Signal<u32>,
    enter: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .absolute()
        .left(px(12.0))
        .right(px(12.0))
        // 横条贴在控制栏上方、信息层之下（Tauri bottom-14 同位）。
        .bottom(px(56.0))
        .h(px(40.0))
        .items_center()
        .justify_center()
        .flex_row()
        .gap(px(6.0))
        .rounded(px(8.0))
        .bg(rgba(0x00000099))
        .child(
            text(move || {
                let count = episode_count.get();
                if count > 0 {
                    format!(
                        "{} · {}",
                        t("player.watchFullSeries"),
                        t("player.totalEpisodes").replace("{count}", &count.to_string())
                    )
                } else {
                    t("player.watchFullSeries")
                }
            })
            .text_size(px(13.0))
            .text_color(rgb(0xffffff)),
        )
        .child(icons::chevron_right(14.0, || rgba(0xffffff99)))
        .on_press(move |_: &()| enter())
        .opacity(move || if chrome_shown.get() { 1.0 } else { 0.0_f32 })
}

/// 暂停时中部露出的快捷键提示胶囊（小窗/竖屏不渲染，由调用方裁决）。
pub(crate) fn keyboard_hint(
    paused: impl Fn() -> bool + 'static,
    stage_h: Signal<f32>,
) -> impl IntoElement {
    If::new(paused, move || {
        view()
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .top(move || px(stage_h.get() * 0.38))
            .items_center()
            // 纯提示，穿透。
            .pointer_events(PointerEvents::None)
            .child(
                text(move || t("player.keyboardHint"))
                    .text_size(px(12.0))
                    .text_color(rgba(0xffffffbf))
                    .px(px(16.0))
                    .py(px(6.0))
                    .rounded_full()
                    .bg(rgba(0x0000008c)),
            )
    })
}

/// 顶部加载细条（hg-loadbar）：切换在途/首帧未出/缓冲中亮起。
/// 30% 宽的亮条在轨道上循环横扫（任务驱动原生位移，无引擎每帧工作；
/// 隐藏时任务挂起不再产帧）。
pub(crate) fn loadbar(shown: impl Fn() -> bool + Clone + 'static) -> impl IntoElement {
    let bar_x = native_value(-120.0);
    let runner = bar_x.clone();
    let alive = shown.clone();
    spawn(async move {
        loop {
            if alive() {
                let _ = runner.clone().timing(1000.0, 1200.0, Easing::Linear).await;
                runner.set(-120.0);
            } else {
                timer(std::time::Duration::from_millis(500)).await;
            }
        }
    });
    If::new(shown, move || {
        view()
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .top(px(0.0))
            .h(px(2.0))
            .overflow_hidden()
            // 纯反馈细条，穿透。
            .pointer_events(PointerEvents::None)
            .bg(rgba(0xffffff26))
            .child({
                let bar_x = bar_x.clone();
                view()
                    .w(px(120.0))
                    .h_full()
                    .rounded(px(1.0))
                    .bg(rgb(0xef4444))
                    .bind_translate_x(&bar_x)
            })
    })
}

/// 右上角错误胶囊（跟随悬浮层显隐；红底描边，Tauri `error-bar.tsx` 同款）。
pub(crate) fn error_bar(chrome_shown: Signal<bool>, error: Signal<String>) -> impl IntoElement {
    If::new(
        move || chrome_shown.get() && !error.get().is_empty(),
        move || {
            view()
                .absolute()
                .top(px(12.0))
                .right(px(12.0))
                .px(px(12.0))
                .py(px(4.0))
                .rounded_full()
                .border_1()
                .border_color(rgba(0xef44444d))
                .bg(rgba(0x450a0ae6))
                // 纯展示胶囊，穿透（右上角不挡舞台）。
                .pointer_events(PointerEvents::None)
                .child(
                    text(move || error.get())
                        .text_size(px(12.0))
                        .text_color(rgba(0xfecacadd)),
                )
        },
    )
}

/// 中央加载面板：无流时压在封面上的「正在准备/缓冲 x%」胶囊 + 错误重试。
pub(crate) fn loading_panel(
    error: Signal<String>,
    status: impl Fn() -> String + Clone + 'static,
    on_retry: impl Fn() + Clone + 'static,
) -> impl IntoElement {
    view()
        .absolute()
        .left(px(0.0))
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .items_center()
        .justify_center()
        .flex_col()
        .gap(px(12.0))
        // 容器穿透（四边空白不挡舞台），重试按钮作为子视图照常可点。
        .pointer_events(PointerEvents::BoxNone)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(16.0))
                .py(px(6.0))
                .rounded_full()
                .bg(rgba(0x00000099))
                .child(If::new(
                    move || error.get().is_empty(),
                    move || {
                        let status = status.clone();
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(icons::spinner(14.0, || rgba(0xffffffd9)).animate_spin())
                            .child(
                                text(status)
                                    .text_size(px(12.0))
                                    .text_color(rgba(0xffffffd9)),
                            )
                    },
                ))
                .child(If::new(
                    move || !error.get().is_empty(),
                    move || {
                        text(move || error.get())
                            .text_size(px(12.0))
                            .text_color(rgba(0xffffffd9))
                    },
                )),
        )
        .child(If::new(
            move || !error.get().is_empty(),
            move || {
                let on_retry = on_retry.clone();
                text(move || t("player.retry"))
                    .text_size(px(13.0))
                    .text_color(rgb(0xffffff))
                    .px(px(16.0))
                    .py(px(6.0))
                    .rounded_full()
                    .border_1()
                    .border_color(rgba(0xffffff4d))
                    .on_press(move |_: &()| on_retry())
            },
        ))
}

/// 中央 spinner（换集/首帧间隙；此刻必然还没有画面）。
pub(crate) fn center_spinner(shown: impl Fn() -> bool + 'static) -> impl IntoElement {
    If::new(shown, move || {
        view()
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .right(px(0.0))
            .bottom(px(0.0))
            .items_center()
            .justify_center()
            // 纯反馈，穿透。
            .pointer_events(PointerEvents::None)
            .child(icons::spinner(32.0, || rgba(0xffffff99)).animate_spin())
    })
}
