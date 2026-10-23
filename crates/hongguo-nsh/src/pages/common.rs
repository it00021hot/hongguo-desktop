//! 账号域页面共用件：阶段态占位 / 筛选胶囊 / 卡面封面。
//!
//! 阶段态位置对齐 Tauri 的 `ErrorState`/`Empty`（流内块、顶部对齐、py-16），
//! 不是垂直居中。

use nsh_core::ui::prelude::*;

use crate::i18n::t;
use crate::player::icons;

/// 列表页阶段。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Phase {
    Loading,
    Failed(String),
    /// 未登录（收藏 / 追更是服务端列表；历史用本地模式不算这态）。
    LoginRequired,
    Loaded,
}

/// 页面内容页边距（Tauri 列表页 p-4 同档）。
pub(crate) const PAGE_PAD: f32 = 16.0;

/// 加载态：居中 spinner。
pub(crate) fn loading_body() -> impl IntoElement {
    view()
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .child(icons::spinner(24.0, crate::tokens::icon_muted))
}

/// 失败态：说明 + 原文 + 重试。
pub(crate) fn failed_body(message: Signal<String>, retry: impl Fn() + 'static) -> impl IntoElement {
    view()
        .w_full()
        .flex_col()
        .items_center()
        .gap(px(12.0))
        .pt(px(64.0))
        .child(
            text(move || t("common.loadFailed"))
                .text_size(px(14.0))
                .text_color(theme().muted_foreground),
        )
        .child(
            text(move || message.get())
                .text_size(px(12.0))
                .text_color(theme().muted_foreground),
        )
        .child(pill(
            "retry",
            move || t("common.retry"),
            move || theme().muted,
            move || theme().foreground,
            retry,
        ))
}

/// 登录墙：说明 + 登录入口（壳级登录面板）。
pub(crate) fn login_body(hint_key: &'static str) -> impl IntoElement {
    view()
        .w_full()
        .flex_col()
        .items_center()
        .gap(px(12.0))
        .pt(px(64.0))
        .child(
            text(move || t(hint_key))
                .text_size(px(14.0))
                .text_color(theme().muted_foreground),
        )
        .child(pill(
            "login-entry",
            move || t("settings.loginAction"),
            move || theme().primary,
            move || theme().primary_foreground,
            crate::account::open_login,
        ))
}

/// 空态：主句 + 补充说明（tab 过滤空态没有补充词条，传 `None`）。
pub(crate) fn empty_body(key: &'static str, hint_key: Option<&'static str>) -> impl IntoElement {
    view()
        .w_full()
        .flex_col()
        .items_center()
        .gap(px(8.0))
        .pt(px(64.0))
        .child(
            text(move || t(key))
                .text_size(px(14.0))
                .text_color(theme().muted_foreground),
        )
        .child(If::new(
            move || hint_key.is_some(),
            move || {
                text(move || t(hint_key.unwrap_or("")))
                    .text_size(px(12.0))
                    .text_color(theme().muted_foreground)
            },
        ))
}

/// 一枚可点的胶囊（选中 = primary 实底；筛选 tab / 动作钮共用一套语言）。
pub(crate) fn pill(
    key: &'static str,
    label: impl Fn() -> String + 'static,
    bg: impl Fn() -> AdaptiveColor + Clone + 'static,
    fg: impl Fn() -> AdaptiveColor + Clone + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    text(label)
        .text_size(px(13.0))
        .text_color(fg)
        .px(px(12.0))
        .py(px(4.0))
        .rounded(px(999.0))
        .bg(bg)
        .key(key)
        .on_press(move |_: &()| on_press())
}
