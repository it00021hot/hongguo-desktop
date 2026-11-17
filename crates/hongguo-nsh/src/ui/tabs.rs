use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn mobile_navigation() -> nsh_base::TabsElement {
    nsh_base::tabs()
        .flex_row()
        .flex_shrink_0()
        .pb(move || safe_area().bottom)
        .bg(move || theme().background)
        .border_t_1()
        .border_color(move || theme().border)
}

pub(crate) fn mobile_navigation_item(
    label: impl IntoReactive<SharedString>,
) -> nsh_base::TabElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    nsh_base::tab()
        .accessibility_label(move || Some(label.get()))
        .flex_1()
        .min_w_0()
        .min_h_12()
        .py_1()
        .items_center()
        .justify_center()
        .rounded(px(0.0))
        .bg(move || theme().background)
        .selected_style(|style| {
            style
                .bg(move || theme().background)
                .text_color(move || theme().primary)
        })
        .text_color(move || theme().muted_foreground)
        .active(|style| style.opacity(0.7))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .child(
            text(move || label.get())
                .text_size(px(15.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn tabs() -> nsh_base::TabsElement {
    nsh_base::tabs()
        .self_start()
        .flex_row()
        .h_9()
        .p_1()
        .rounded(crate::ui::radii::control)
        .bg(move || theme().muted)
}

pub(crate) fn tab(label: impl IntoReactive<SharedString>) -> nsh_base::TabElement {
    nsh_base::tab()
        .min_h(px(if is_desktop() { 28.0 } else { 48.0 }))
        .px_2()
        .py_1()
        .items_center()
        .justify_center()
        .rounded(crate::ui::radii::control)
        .text_color(move || theme().muted_foreground)
        .selected_style(super::palette::selected)
        .hover(super::palette::highlighted)
        .active(|style| style.opacity(0.8))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .disabled_style(|style| style.opacity(0.45))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                crate::ui::motion::FAST_MS
            }
        })
        .child(
            text(label)
                .text_sm()
                .style_when(move || !is_desktop(), |style| style.text_base())
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}
