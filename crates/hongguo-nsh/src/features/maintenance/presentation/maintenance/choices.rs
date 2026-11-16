use super::{MaintenanceIcon, maintenance_icon};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn maintenance_select(
    label: impl IntoReactive<SharedString>,
    options: Memo<Vec<crate::ui::SelectOption>>,
    value: Memo<Option<SharedString>>,
    placeholder: impl IntoReactive<SharedString>,
) -> crate::ui::SelectControl {
    let placeholder = placeholder.into_reactive();
    crate::ui::select(label, options, value).w_full().trigger(
        view()
            .w_full()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
            .px_3()
            .py_1()
            .rounded(crate::ui::radii::control())
            .border_1()
            .border_color(move || theme().input)
            .bg(move || {
                if dark() {
                    theme().input.opacity(0.3)
                } else {
                    theme().background
                }
            })
            .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
            .child(
                text(move || {
                    let selected = value.get();
                    options
                        .with(|options| {
                            options
                                .iter()
                                .find(|item| Some(&item.value) == selected.as_ref())
                                .map(|item| item.title.clone())
                        })
                        .unwrap_or_else(|| placeholder.get())
                })
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_sm()
                .font_weight(FontWeight::NORMAL)
                .text_color(move || {
                    if value.get().is_some() {
                        theme().foreground
                    } else {
                        theme().muted_foreground
                    }
                }),
            )
            .child(
                crate::ui::icon(crate::ui::Icon::Dropdown)
                    .text_color(move || theme().muted_foreground),
            ),
    )
}

pub(crate) fn maintenance_modes() -> nsh_base::TabsElement {
    crate::ui::tabs()
        .w_full()
        .self_stretch()
        .h(move || px(if is_desktop() { 36.0 } else { 56.0 }))
        .p(px(3.0))
        .rounded(move || theme().radius)
        .items_center()
}

pub(crate) fn maintenance_mode(
    label: impl IntoReactive<SharedString>,
    glyph: MaintenanceIcon,
) -> nsh_base::TabElement {
    nsh_base::tab()
        .flex_1()
        .min_h(move || px(if is_desktop() { 29.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 29.0 } else { 48.0 }))
        .px_2()
        .py_1()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_1p5()
        .rounded(crate::ui::radii::control())
        .border_1()
        .border_color(move || theme().border.opacity(0.0))
        .text_color(move || theme().foreground.opacity(0.7))
        .selected_style(|style| {
            style
                .bg(move || theme().background)
                .text_color(move || theme().foreground)
                .shadow_sm()
        })
        .disabled_style(|style| style.opacity(0.5))
        .hover(|style| style.bg(move || theme().accent))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                crate::ui::motion::FAST_MS
            }
        })
        .child(maintenance_icon(glyph))
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM))
}
