//! Shared visual style for page-level filter and category choices.
use nsh_base::theme;
use nsh_core::{
    element::{ComponentBuilder, StyleRefinement, Styled},
    prelude::*,
};

use super::{controls, motion, palette};

pub(crate) fn filter_pill(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    pill(nsh_base::button())
        .selected_style(selected)
        .child(text(label).text_sm().line_height(px(20.0)))
}

pub(crate) fn filter_toggle(label: impl IntoReactive<SharedString>) -> nsh_base::ToggleElement {
    pill(nsh_base::toggle())
        .pressed_style(selected)
        .child(text(label).text_sm().line_height(px(20.0)))
}

fn pill<T: Styled + ComponentBuilder>(element: T) -> T {
    element
        .min_h(controls::control_height)
        .h(controls::control_height)
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .px_4()
        .py_1p5()
        .rounded_full()
        .bg(move || theme().muted)
        .text_color(move || theme().muted_foreground)
        .font_weight(FontWeight::NORMAL)
        .hover(|style| {
            palette::highlighted(style)
                .selected(selected)
                .aria_pressed(selected)
        })
        .active(|style| {
            style.opacity(0.8).scale(move || {
                if reduce_motion() {
                    1.0
                } else {
                    motion::PRESS_SCALE
                }
            })
        })
        .disabled_style(palette::disabled)
        .focus_visible(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                motion::FAST_MS
            }
        })
        .ease_out()
}

fn selected(style: StyleRefinement) -> StyleRefinement {
    palette::selected(style)
        .border(px(0.0))
        .font_weight(FontWeight::MEDIUM)
}
