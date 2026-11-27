use crate::ui::motion::FAST_MS;
use crate::ui::motion::PRESS_SCALE;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) const DESKTOP_CONTROL_HEIGHT: f32 = 36.0;
pub(crate) const COMPACT_CONTROL_HEIGHT: f32 = 32.0;

pub(crate) fn control_height() -> Points {
    px(if is_desktop() {
        DESKTOP_CONTROL_HEIGHT
    } else {
        48.0
    })
}

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
    surface(variant).h(px(DESKTOP_CONTROL_HEIGHT)).child(
        text(label)
            .text_sm()
            .line_height(px(20.0))
            .font_weight(FontWeight::MEDIUM),
    )
}

pub(crate) fn compact_button(
    label: impl IntoReactive<SharedString>,
    variant: ButtonVariant,
) -> nsh_base::ButtonElement {
    surface_height(variant, COMPACT_CONTROL_HEIGHT)
        .h(px(COMPACT_CONTROL_HEIGHT))
        .py_0()
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn surface(variant: ButtonVariant) -> nsh_base::ButtonElement {
    surface_height(variant, DESKTOP_CONTROL_HEIGHT)
}

pub(crate) fn surface_height(variant: ButtonVariant, height: f32) -> nsh_base::ButtonElement {
    nsh_base::button()
        .min_h(px(height))
        .hit_slop(EdgeInsets::all(px(if is_desktop() {
            0.0
        } else {
            ((48.0 - height) * 0.5).max(0.0)
        })))
        .px(px(if is_desktop() { 16.0 } else { 12.0 }))
        .py(px(if is_desktop() { 8.0 } else { 4.0 }))
        .items_center()
        .justify_center()
        .rounded(crate::ui::radii::control())
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
            style
                .bg(move || match variant {
                    ButtonVariant::Primary => theme().primary.opacity(0.9),
                    _ => theme().accent,
                })
                .selected(super::palette::selected)
        })
        .active(|style| {
            style
                .opacity(0.8)
                .scale(move || if reduce_motion() { 1.0 } else { PRESS_SCALE })
        })
        .selected_style(super::palette::selected)
        .disabled_style(super::palette::disabled)
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
        .min_h(control_height)
        .h(control_height)
        .px_3()
        .py_2()
        .text_sm()
        .rounded(crate::ui::radii::control())
        .border_1()
        .border_color(move || theme().input)
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || theme().foreground)
        .focus_visible(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
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
                crate::ui::motion::CHROME_MS
            }
        })
}
