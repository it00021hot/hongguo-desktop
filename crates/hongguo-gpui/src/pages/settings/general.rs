//! 通用设置卡：命名与并发、更新，以及命名与代理的标签和索引换算
//! （`new` / `refresh_locale` / `load` / `save` 共用）。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::h_flex;
use gpui_kit::component::label::Label;
use gpui_kit::component::select::Select;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::div;
use hongguo_common::model::settings::{NamingTemplate, ProxyMode};

use crate::pages::card;
use crate::ui_button::button as Button;

pub(super) fn naming_labels() -> [String; 3] {
    [
        crate::i18n::t("settings.namingTitleIndex"),
        crate::i18n::t("settings.namingTitleIndexEpisode"),
        crate::i18n::t("settings.namingOnlyTitle"),
    ]
}

pub(super) fn proxy_labels() -> [String; 3] {
    [
        crate::i18n::t("settings.proxySystem"),
        crate::i18n::t("settings.proxyManual"),
        crate::i18n::t("settings.proxyDirect"),
    ]
}

pub(super) fn naming_items() -> SearchableVec<SharedString> {
    SearchableVec::new(
        naming_labels()
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<SharedString>>(),
    )
}

pub(super) fn proxy_items() -> SearchableVec<SharedString> {
    SearchableVec::new(
        proxy_labels()
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<SharedString>>(),
    )
}

pub(super) fn naming_index(naming: NamingTemplate) -> usize {
    match naming {
        NamingTemplate::TitleIndex => 0,
        NamingTemplate::TitleIndexEpisode => 1,
        NamingTemplate::OnlyTitle => 2,
    }
}

pub(super) fn naming_from_index(ix: usize) -> NamingTemplate {
    match ix {
        1 => NamingTemplate::TitleIndexEpisode,
        2 => NamingTemplate::OnlyTitle,
        _ => NamingTemplate::TitleIndex,
    }
}

pub(super) fn proxy_index(mode: ProxyMode) -> usize {
    match mode {
        ProxyMode::System => 0,
        ProxyMode::Manual => 1,
        ProxyMode::Direct => 2,
    }
}

pub(super) fn proxy_from_index(ix: usize) -> ProxyMode {
    match ix {
        1 => ProxyMode::Manual,
        2 => ProxyMode::Direct,
        _ => ProxyMode::System,
    }
}

impl super::SettingsView {
    pub(super) fn render_naming_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let current = self.current();
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(crate::i18n::t("settings.naming")),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(crate::i18n::t("settings.naming")))
                    .child(Select::new(&self.naming_select).w_full()),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(format!(
                        "{} — {}",
                        crate::i18n::t("settings.concurrency"),
                        current.max_concurrency
                    )))
                    .child(Slider::new(&self.concurrency))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("settings.concurrencyDesc")),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(format!(
                        "{} — {}",
                        crate::i18n::t("settings.prefetchEpisodes"),
                        current.prefetch_episodes
                    )))
                    .child(Slider::new(&self.prefetch))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("settings.prefetchEpisodesDesc")),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_update_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        card(cx)
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(crate::i18n::t("update.appTitle")),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("settings-check-update")
                            .label(crate::i18n::t("update.checkUpdate"))
                            .outline()
                            .small()
                            .disabled(true)
                            .cursor_default(),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(crate::i18n::tf(
                        "settings.gpuiUpdateUnavailable",
                        &[("version", env!("CARGO_PKG_VERSION"))],
                    ))),
            )
            .into_any_element()
    }
}
