use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn tabs() -> nsh_base::TabsElement {
    nsh_base::tabs()
        .self_start()
        .flex_row()
        .h_9()
        .p_1()
        .rounded(super::radii::control)
        .bg(move || theme().muted)
}

pub(crate) fn tab(label: impl IntoReactive<SharedString>) -> nsh_base::TabElement {
    nsh_base::tab()
        .min_h(px(if is_desktop() { 28.0 } else { 48.0 }))
        .px_2()
        .py_1()
        .items_center()
        .justify_center()
        .rounded(super::radii::control)
        .text_color(move || theme().muted_foreground)
        .selected_style(|style| {
            style
                .bg(move || theme().background)
                .text_color(move || theme().foreground)
        })
        .hover(|style| style.bg(move || theme().accent))
        .active(|style| style.opacity(0.8))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .disabled_style(|style| style.opacity(0.45))
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
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}
