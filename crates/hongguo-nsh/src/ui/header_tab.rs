use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn header_tab(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .min_h(px(24.0))
        .h_6()
        .px_3()
        .py_1()
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || theme().muted_foreground)
        .selected_style(|style| {
            style
                .bg(move || theme().primary)
                .text_color(move || theme().primary_foreground)
        })
        .hover(|style| style.bg(move || theme().accent))
        .active(|style| style.opacity(0.75))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL),
        )
}
