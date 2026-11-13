use super::motion::{FAST_MS, PRESS_SCALE};
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) enum ButtonVariant {
    Primary,
    Secondary,
    Quiet,
}

pub(crate) fn button(
    label: impl IntoReactive<SharedString>,
    variant: ButtonVariant,
) -> nsh_base::ButtonElement {
    surface(variant).child(text(label).text_sm().font_weight(FontWeight::MEDIUM))
}

pub(super) fn surface(variant: ButtonVariant) -> nsh_base::ButtonElement {
    surface_height(variant, 36.0)
}

pub(super) fn surface_height(
    variant: ButtonVariant,
    desktop_height: f32,
) -> nsh_base::ButtonElement {
    nsh_base::button()
        .min_h(px(if is_desktop() { desktop_height } else { 48.0 }))
        .px_4()
        .py_2()
        .items_center()
        .justify_center()
        .rounded(super::radii::control())
        .bg(move || match variant {
            ButtonVariant::Primary => theme().primary,
            ButtonVariant::Secondary => theme().secondary,
            ButtonVariant::Quiet => theme().background.opacity(0.0),
        })
        .text_color(move || match variant {
            ButtonVariant::Primary => theme().primary_foreground,
            _ => theme().foreground,
        })
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
        .ease_out()
        .hover(move |style| {
            style.bg(move || match variant {
                ButtonVariant::Primary => theme().primary.opacity(0.9),
                _ => theme().accent,
            })
        })
        .active(|style| {
            style
                .opacity(0.8)
                .scale(move || if reduce_motion() { 1.0 } else { PRESS_SCALE })
        })
        .selected_style(|style| {
            style
                .bg(move || theme().primary.opacity(0.12))
                .text_color(move || theme().foreground)
                .border_1()
                .border_color(move || theme().foreground.opacity(0.24))
        })
        .disabled_style(|style| style.opacity(0.45))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
}

pub(crate) fn field(label: impl IntoReactive<SharedString>) -> nsh_base::Input {
    let label = label.into_reactive();
    let name = label.clone();
    nsh_base::input()
        .placeholder(move || label.get().to_string())
        .placeholder_color(move || color(theme().muted_foreground))
        .caret_color(move || color(theme().ring))
        .selection_color(move || color(theme().primary.opacity(0.3)))
        .label(move || name.get())
        .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .px_3()
        .py_2()
        .text_sm()
        .rounded(super::radii::control())
        .border_1()
        .border_color(move || theme().input)
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || theme().foreground)
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
}

pub(crate) fn status(message: impl IntoReactive<SharedString>) -> View {
    let message = message.into_reactive();
    let visible = message.clone();
    view()
        .py_3()
        .gap_3()
        .style_when(move || visible.get().is_empty(), |style| style.hidden())
        .child(
            text(move || message.get())
                .text_sm()
                .text_color(move || theme().muted_foreground),
        )
}

pub(crate) fn form(content: impl IntoElement) -> View {
    form_with_scroll(content, scroll())
}

pub(crate) fn form_with_scroll(content: impl IntoElement, scroller: Scroll) -> View {
    view()
        .flex_1()
        .pb_keyboard()
        .bg(move || theme().background)
        .child(
            scroller
                .test_id("form-scroll")
                .flex_1()
                .bg(move || theme().background)
                .keyboard_taps(nsh_core::KeyboardTaps::Handled)
                .keyboard_dismiss_mode(nsh_core::KeyboardDismissMode::OnDrag)
                .child(content),
        )
}

pub(crate) fn refresh_shade(refreshing: impl IntoReactive<bool>) -> View {
    let refreshing = refreshing.into_reactive();
    let refreshing = computed(move || refreshing.get());
    view()
        .flex_1()
        .min_h(px(0.0))
        .opacity(move || if refreshing.get() { 0.5_f32 } else { 1.0_f32 })
        .pointer_events(move || {
            if refreshing.get() {
                PointerEvents::None
            } else {
                PointerEvents::Auto
            }
        })
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::CHROME_MS
            }
        })
}
