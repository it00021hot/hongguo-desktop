use super::{ButtonVariant, Icon, controls::surface_height, icon};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn batch_bar() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .mx(px(-16.0))
        .px_4()
        .py_3()
        .border_t_1()
        .border_color(move || theme().border)
        .bg(move || theme().background.opacity(0.95))
}

pub(crate) fn batch_select(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    surface_height(ButtonVariant::Quiet, 32.0)
        .flex_row()
        .gap_2()
        .px_3()
        .py_1()
        .child(icon(Icon::Tasks))
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn batch_delete(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    surface_height(ButtonVariant::Quiet, 32.0)
        .flex_row()
        .gap_2()
        .px_3()
        .py_1()
        .bg(move || theme().destructive.opacity(if dark() { 0.6 } else { 1.0 }))
        .text_color(move || {
            theme()
                .color("destructive-foreground")
                .expect("destructive foreground token")
        })
        .hover(|style| style.bg(move || theme().destructive.opacity(0.9)))
        .child(icon(Icon::Delete))
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}
