use super::{ButtonVariant, Icon, controls::surface, icon, radii};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn detail_section() -> View {
    view().w_full().max_w(px(1152.0)).self_center().px(move || {
        px(if window().width.0 >= 768.0 {
            32.0
        } else {
            16.0
        })
    })
}

pub(crate) fn detail_action(
    label: impl IntoReactive<SharedString>,
    glyph: Icon,
    primary: bool,
) -> nsh_base::ButtonElement {
    detail_surface(primary).child(icon(glyph)).child(
        text(label)
            .text_sm()
            .line_height(px(20.0))
            .font_weight(FontWeight::MEDIUM),
    )
}

fn detail_surface(primary: bool) -> nsh_base::ButtonElement {
    surface(if primary {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Quiet
    })
    .min_h(px(32.0))
    .h_8()
    .px_2p5()
    .py_1()
    .flex_row()
    .gap_1p5()
    .border_1()
    .border_color(move || {
        if primary {
            theme().primary
        } else {
            theme().border
        }
    })
}

pub(crate) fn reserve_action(
    selected: impl IntoReactive<bool>,
    pending: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let selected = selected.into_reactive();
    let selected = computed(move || selected.get());
    let pending = pending.into_reactive();
    detail_surface(false)
        .gap_1()
        .border_color(move || theme().input)
        .bg(move || {
            if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .hover(|style| {
            style.bg(move || {
                if dark() {
                    theme().input.opacity(0.5)
                } else {
                    theme().accent
                }
            })
        })
        .shadow_xs()
        .disabled(move || pending.get())
        .child(dynamic(move || {
            icon(if selected.get() {
                Icon::Check
            } else {
                Icon::Bell
            })
        }))
        .child(
            text(move || {
                crate::i18n::tr(if selected.get() {
                    "player.reserved"
                } else {
                    "player.reserve"
                })
            })
            .text_sm()
            .line_height(px(20.0))
            .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn detail_badge(label: impl IntoReactive<SharedString>, highlighted: bool) -> View {
    view()
        .self_start()
        .px_2()
        .py_0p5()
        .border_1()
        .border_color(move || theme().border.opacity(0.0))
        .rounded(radii::control)
        .bg(move || {
            if highlighted {
                theme().color("hot").expect("hot token")
            } else {
                theme().secondary
            }
        })
        .text_color(move || {
            if highlighted {
                theme().primary_foreground
            } else {
                theme().secondary_foreground
            }
        })
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn detail_inline(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .min_h(px(20.0))
        .h_5()
        .p_0()
        .self_start()
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::NORMAL),
        )
}

pub(crate) fn episode_card(
    index: impl IntoReactive<u32>,
    duration: impl IntoReactive<i64>,
) -> nsh_base::ButtonElement {
    let index = index.into_reactive();
    let duration = duration.into_reactive();
    let number = index.clone();
    if window().width.0 < 768.0 {
        return surface(ButtonVariant::Secondary)
            .min_h(px(48.0))
            .p_0()
            .bg(move || theme().muted)
            .rounded(move || theme().radius)
            .font_weight(FontWeight::NORMAL)
            .selected_style(|style| {
                style
                    .bg(move || theme().primary)
                    .text_color(move || theme().primary_foreground)
                    .font_weight(FontWeight::SEMIBOLD)
                    .border(px(0.0))
            })
            .aspect_ratio(1.0)
            .child(text(move || number.get().to_string()).text_sm());
    }
    surface(ButtonVariant::Quiet)
        .min_h(px(if is_desktop() { 42.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 42.0 } else { 48.0 }))
        .px_4()
        .py_2()
        .flex_row()
        .gap_3()
        .justify_start()
        .bg(move || theme().card)
        .border_1()
        .border_color(move || theme().border)
        .selected_style(|style| {
            style
                .border_color(move || theme().primary)
                .text_color(move || theme().primary)
        })
        .child(
            text(move || number.get().to_string())
                .w_8()
                .text_center()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD),
        )
        .child(
            text(move || {
                crate::i18n::tr("player.epShort").replace("{index}", &index.get().to_string())
            })
            .text_sm()
            .font_weight(FontWeight::NORMAL),
        )
        .child(view().flex_1())
        .child(
            text(move || {
                let seconds = duration.get();
                if seconds <= 0 {
                    String::new()
                } else {
                    format!("{:02}:{:02}", seconds / 60, seconds % 60)
                }
            })
            .text_xs()
            .text_color(move || theme().muted_foreground)
            .font_weight(FontWeight::NORMAL),
        )
}
