use super::{motion::FAST_MS, radii};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) enum KeyBindingKind {
    Player,
    Boss,
}

pub(crate) fn settings_key(
    label: impl IntoReactive<SharedString>,
    capturing: impl IntoReactive<bool>,
    kind: KeyBindingKind,
) -> nsh_base::ButtonElement {
    let capturing = capturing.into_reactive();
    let capturing = computed(move || capturing.get());
    nsh_base::button()
        .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .min_w(px(match kind {
            KeyBindingKind::Player => 96.0,
            KeyBindingKind::Boss => 128.0,
        }))
        .px_3()
        .py_0()
        .items_center()
        .justify_center()
        .rounded(radii::control)
        .border_1()
        .border_color(move || {
            if capturing.get() {
                theme().primary
            } else {
                theme().input
            }
        })
        .bg(move || {
            if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .text_color(move || {
            if capturing.get() {
                theme().primary
            } else {
                theme().foreground
            }
        })
        .hover(|style| style.bg(move || theme().accent))
        .active(|style| style.opacity(0.8))
        .disabled_style(|style| style.opacity(0.5))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_family("monospace")
                .font_weight(FontWeight::MEDIUM),
        )
}
