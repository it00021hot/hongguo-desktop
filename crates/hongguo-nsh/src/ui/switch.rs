use super::motion::FAST_MS;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn switch(label: impl IntoReactive<SharedString>) -> nsh_base::SwitchElement {
    let label = label.into_reactive();
    let name = label.clone();
    nsh_base::switch()
        .accessibility_label(move || name.get())
        .flex_row()
        .items_center()
        .gap_3()
        .min_h(move || px(if is_desktop() { 18.4 } else { 48.0 }))
        .text_color(move || theme().foreground)
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .disabled_style(|style| style.opacity(0.45))
        .track_style(|style| {
            style
                .w(move || px(if is_desktop() { 32.0 } else { 44.0 }))
                .h(move || px(if is_desktop() { 18.4 } else { 24.0 }))
                .p(move || px(if is_desktop() { 0.0 } else { 1.0 }))
                .justify_center()
                .flex_shrink_0()
                .rounded_full()
                .border_1()
                .border_color(move || theme().background.opacity(0.0))
                .bg(move || theme().input)
                .transition()
                .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
                .ease_out()
                .checked(|style| style.bg(move || theme().primary))
        })
        .thumb_style(|style| {
            style
                .size(move || px(if is_desktop() { 16.0 } else { 20.0 }))
                .flex_shrink_0()
                .rounded_full()
                .bg(move || theme().background)
                .transition()
                .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
                .ease_out()
                .checked(|style| {
                    style.translate_x(move || px(if is_desktop() { 14.0 } else { 20.0 }))
                })
        })
        .child(
            text(move || label.get())
                .text_size(move || px(if is_desktop() { 14.0 } else { 16.0 }))
                .line_height(move || px(if is_desktop() { 14.0 } else { 24.0 }))
                .font_weight(FontWeight::MEDIUM),
        )
}
