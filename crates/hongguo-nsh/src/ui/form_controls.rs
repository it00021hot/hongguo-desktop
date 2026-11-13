use super::{ButtonVariant, Icon, button, field, icon, radii};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) enum FormAction {
    Login,
    Directory,
    TestProxy,
}

pub(crate) fn form_icon_action(
    label: impl IntoReactive<SharedString>,
    action: FormAction,
) -> nsh_base::ButtonElement {
    let (variant, markup, height) = match action {
        FormAction::Login => (
            ButtonVariant::Primary,
            include_str!("icons/log-in.svg"),
            32.0,
        ),
        FormAction::Directory => (
            ButtonVariant::Secondary,
            include_str!("icons/folder-open.svg"),
            36.0,
        ),
        FormAction::TestProxy => (
            ButtonVariant::Secondary,
            include_str!("icons/zap.svg"),
            32.0,
        ),
    };
    let button = super::controls::surface(variant)
        .flex_row()
        .items_center()
        .gap_2()
        .min_h(move || px(if is_desktop() { height } else { 48.0 }))
        .px_3()
        .py_1p5()
        .child(
            svg(nsh_core::element::ImageSrc::svg_markup(markup))
                .size(px(16.0))
                .fill_none()
                .stroke_current()
                .stroke_width(px(2.0)),
        )
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM));
    if matches!(action, FormAction::Directory) {
        button
            .bg(move || {
                if dark() {
                    theme().input.opacity(0.3)
                } else {
                    theme().background
                }
            })
            .border_1()
            .border_color(move || theme().input)
            .hover(|style| {
                style.bg(move || {
                    if dark() {
                        theme().input.opacity(0.5)
                    } else {
                        theme().accent
                    }
                })
            })
    } else {
        button
    }
}

pub(crate) fn form_label(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_sm()
        .line_height(nsh_core::LineHeight::Multiple(1.0))
        .font_weight(FontWeight::MEDIUM)
}

pub(crate) fn form_card(title: impl IntoReactive<SharedString>) -> View {
    form_surface(text(title).text_base().font_weight(FontWeight::SEMIBOLD))
}

pub(crate) fn form_surface(header: impl IntoElement) -> View {
    view()
        .gap_6()
        .p_6()
        .bg(move || theme().card)
        .rounded(radii::card)
        .border_1()
        .border_color(move || theme().border)
        .shadow_sm()
        .child(header)
}

pub(crate) fn form_description(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_xs()
        .text_color(move || theme().muted_foreground)
}

pub(crate) fn form_action(
    label: impl IntoReactive<SharedString>,
    variant: ButtonVariant,
) -> nsh_base::ButtonElement {
    button(label, variant)
        .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .px_3()
        .py_1p5()
}

pub(crate) fn form_save(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    button(label, ButtonVariant::Primary)
        .h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .px_4()
        .py_0()
}

pub(crate) fn directory_field(label: impl IntoReactive<SharedString>) -> nsh_base::Input {
    field(label)
        .text_xs()
        .font_weight(FontWeight::NORMAL)
        .font_family("monospace")
}

pub(crate) fn form_hint(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_sm()
        .text_color(move || theme().muted_foreground)
}

pub(crate) fn form_separator() -> View {
    view()
        .w_full()
        .h(px(1.0))
        .flex_shrink_0()
        .bg(move || theme().border)
}

pub(crate) fn account_title() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(icon(Icon::Account))
        .child(
            text(move || crate::i18n::tr("settings.account"))
                .text_base()
                .font_weight(FontWeight::SEMIBOLD),
        )
}

pub(crate) fn update_action(
    label: impl IntoReactive<SharedString>,
    available: Memo<bool>,
) -> nsh_base::ButtonElement {
    form_action(label, ButtonVariant::Quiet)
        .h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .py_0()
        .border_1()
        .border_color(move || {
            if available.get() {
                theme().primary
            } else if dark() {
                theme().input
            } else {
                theme().border
            }
        })
        .bg(move || {
            if available.get() {
                theme().primary
            } else if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .text_color(move || {
            if available.get() {
                theme().primary_foreground
            } else {
                theme().foreground
            }
        })
        .hover(|style| {
            style.bg(move || {
                if available.get() {
                    theme().primary.opacity(0.9)
                } else if dark() {
                    theme().input.opacity(0.5)
                } else {
                    theme().accent
                }
            })
        })
}
