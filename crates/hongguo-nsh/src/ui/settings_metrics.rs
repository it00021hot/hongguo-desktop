use super::{Icon, icon, motion::FAST_MS, radii};
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BackendLevel {
    Hardware,
    Software,
    Unavailable,
}

pub(crate) fn settings_note() -> View {
    view()
        .gap_2()
        .p_3()
        .rounded(radii::control)
        .bg(move || theme().muted.opacity(0.5))
}

pub(crate) fn backend_badge(
    label: impl IntoReactive<SharedString>,
    level: impl IntoReactive<BackendLevel>,
) -> View {
    let level = level.into_reactive();
    let level = computed(move || level.get());
    view()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .px_2()
        .py_0p5()
        .rounded(radii::control)
        .border_1()
        .border_color(move || theme().background.opacity(0.0))
        .bg(move || match level.get() {
            BackendLevel::Hardware => theme()
                .color("success")
                .expect("success token")
                .opacity(0.15),
            BackendLevel::Software => theme()
                .color("warning")
                .expect("warning token")
                .opacity(0.15),
            BackendLevel::Unavailable => theme().secondary,
        })
        .text_color(move || match level.get() {
            BackendLevel::Hardware => theme().color("success").expect("success token"),
            BackendLevel::Software => theme().color("warning").expect("warning token"),
            BackendLevel::Unavailable => theme().secondary_foreground,
        })
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn settings_glyph_action(
    label: impl IntoReactive<SharedString>,
    glyph: Icon,
) -> nsh_base::ButtonElement {
    super::controls::surface_height(super::ButtonVariant::Quiet, 32.0)
        .flex_row()
        .gap_2()
        .px_3()
        .py_1p5()
        .flex_shrink_0()
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
        .child(icon(glyph))
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}
