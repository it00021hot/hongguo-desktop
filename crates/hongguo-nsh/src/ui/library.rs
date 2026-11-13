//! Compact history cards and download tables, matching the shared Tauri surfaces.
use super::{ButtonVariant, Icon, controls::surface, icon, radii};
use hongguo_common::model::TaskStatus;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn library_action(
    label: impl IntoReactive<SharedString>,
    glyph: Option<Icon>,
) -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .flex_row()
        .gap_1p5()
        .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .px(move || px(if glyph.is_some() { 10.0 } else { 12.0 }))
        .py_1()
        .flex_shrink_0()
        .bg(move || {
            if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .border_1()
        .border_color(move || {
            if dark() {
                theme().input
            } else {
                theme().border
            }
        })
        .children(glyph.map(icon))
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn library_icon(
    glyph: Icon,
    label: impl IntoReactive<SharedString>,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    surface(ButtonVariant::Quiet)
        .accessibility_label(move || Some(label.get()))
        .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .size(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .p_0()
        .flex_shrink_0()
        .child(icon(glyph))
}

pub(crate) fn history_filter(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .min_h(move || px(if is_desktop() { 24.0 } else { 48.0 }))
        .px_3()
        .py_1()
        .rounded_full()
        .text_color(move || theme().muted_foreground)
        .selected_style(|style| {
            style
                .bg(move || theme().primary)
                .text_color(move || theme().primary_foreground)
        })
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL),
        )
}

pub(crate) fn history_card() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_4()
        .p_3()
        .rounded(radii::card)
        .bg(move || theme().card)
        .border_1()
        .border_color(move || theme().border)
        .hover(|style| style.border_color(move || theme().foreground.opacity(0.3)))
}

pub(crate) fn history_open() -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .flex_row()
        .flex_1()
        .min_w(px(0.0))
        .gap_4()
        .p_0()
        .justify_start()
        .active(|style| style.scale(1.0))
        .hover(|style| style.bg(move || theme().card))
}

pub(crate) fn history_progress(label: impl IntoReactive<SharedString>) -> View {
    view()
        .self_start()
        .px_1p5()
        .py_0p5()
        .rounded(px(4.0))
        .bg(move || theme().primary.opacity(0.1))
        .text_color(move || theme().primary)
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn library_search(
    value: Signal<String>,
    placeholder: impl IntoReactive<SharedString>,
) -> View {
    view()
        .relative()
        .w_60()
        .max_w(percentage(1.0))
        .child(
            super::field(placeholder)
                .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
                .h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
                .pl_8()
                .py_1()
                .value(value)
                .on_change(move |next: &str| value.set(next.to_owned())),
        )
        .child(
            view()
                .absolute()
                .left(px(10.0))
                .top_0()
                .bottom_0()
                .justify_center()
                .pointer_events(nsh_core::PointerEvents::None)
                .text_color(move || theme().muted_foreground)
                .child(icon(Icon::Search)),
        )
}

pub(crate) fn task_table() -> View {
    view()
        .rounded(radii::card)
        .overflow_hidden()
        .bg(move || theme().card)
        .border_1()
        .border_color(move || theme().border)
}

pub(crate) fn task_batch_bar() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .shadow_lg()
}

pub(crate) fn task_batch_close() -> nsh_base::ButtonElement {
    super::controls::surface_height(ButtonVariant::Quiet, 32.0)
        .accessibility_label(move || Some(crate::i18n::tr("common.cancel")))
        .w(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .p_0()
        .child(icon(Icon::Close))
}

pub(crate) fn task_table_row(header: bool) -> View {
    view()
        .flex_row()
        .items_center()
        .min_h(move || {
            px(if header {
                40.0
            } else if is_desktop() {
                52.0
            } else {
                64.0
            })
        })
        .border_b_1()
        .border_color(move || theme().border)
        .text_color(move || {
            if header {
                theme().muted_foreground
            } else {
                theme().foreground
            }
        })
        .hover(move |style| {
            style.bg(move || {
                if header {
                    theme().card
                } else {
                    theme().muted.opacity(0.5)
                }
            })
        })
}

#[derive(Clone, Copy)]
pub(crate) enum TaskColumn {
    Select,
    Title,
    Episode,
    Status,
    Size,
    Actions,
}

impl TaskColumn {
    // Tauri's intrinsic widths at 480px. Keep its preferred w-32 size column:
    // the native mono glyphs require that width to retain the complete byte unit.
    fn minimum(self) -> f32 {
        match self {
            Self::Select => 24.0,
            Self::Title => 256.0,
            Self::Episode => 44.5625,
            Self::Status => 70.765625,
            Self::Size => 128.0,
            Self::Actions => 92.0,
        }
    }
    pub(crate) fn minimum_table_width() -> f32 {
        [
            Self::Select,
            Self::Title,
            Self::Episode,
            Self::Status,
            Self::Size,
            Self::Actions,
        ]
        .into_iter()
        .map(Self::minimum)
        .sum()
    }
}

pub(crate) fn task_cell(column: TaskColumn, compact: Memo<bool>) -> View {
    let cell = view().p_2().justify_center().flex_shrink_0();
    match column {
        TaskColumn::Title => cell
            .flex_1()
            .min_w(move || px(if compact.get() { column.minimum() } else { 0.0 })),
        _ => cell
            .w(move || {
                px(if compact.get() {
                    column.minimum()
                } else {
                    match column {
                        TaskColumn::Select => 40.0,
                        TaskColumn::Episode => 96.0,
                        TaskColumn::Status => 112.0,
                        TaskColumn::Size => 128.0,
                        TaskColumn::Actions => 92.0,
                        TaskColumn::Title => unreachable!(),
                    }
                })
            })
            .style_when(
                move || compact.get() && matches!(column, TaskColumn::Select),
                |style| style.pr_0(),
            ),
    }
}

pub(crate) fn library_check(
    label: impl IntoReactive<SharedString>,
    checked: impl IntoReactive<bool>,
    mixed: impl IntoReactive<bool>,
) -> nsh_base::CheckboxElement {
    let label = label.into_reactive();
    let checked = checked.into_reactive();
    let checked = computed(move || checked.get());
    let mixed = mixed.into_reactive();
    let mixed = computed(move || mixed.get());
    nsh_base::checkbox()
        .accessibility_label(move || Some(label.get()))
        .checked(checked)
        .indeterminate(mixed)
        .indicator_style(|style| {
            style
                .size(px(16.0))
                .rounded(px(4.0))
                .items_center()
                .justify_center()
                .border_1()
                .border_color(move || theme().input)
                .checked(|style| {
                    style
                        .bg(move || theme().primary)
                        .border_color(move || theme().primary)
                })
                .indeterminate(|style| {
                    style
                        .bg(move || theme().primary)
                        .border_color(move || theme().primary)
                })
        })
        .indicator(
            view()
                .text_color(move || theme().primary_foreground)
                .child(when(
                    move || checked.get() && !mixed.get(),
                    move || icon(Icon::Check).size(px(14.0)),
                ))
                .child(when(mixed, move || {
                    view()
                        .w_2()
                        .h(px(1.0))
                        .bg(move || theme().primary_foreground)
                })),
        )
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .hit_slop(if is_desktop() { 4.0 } else { 16.0 })
}

pub(crate) fn task_badge(status: TaskStatus, label: impl IntoReactive<SharedString>) -> View {
    view()
        .self_start()
        .px_2()
        .py_0p5()
        .rounded(radii::control)
        .border_1()
        .border_color(move || theme().border.opacity(0.0))
        .bg(move || match status {
            TaskStatus::Running => theme().primary,
            TaskStatus::Pending => theme().secondary,
            TaskStatus::Failed => theme().destructive,
            TaskStatus::Completed => theme()
                .color("success")
                .expect("success token")
                .opacity(0.15),
            TaskStatus::Stopped => theme()
                .color("warning")
                .expect("warning token")
                .opacity(0.15),
        })
        .text_color(move || match status {
            TaskStatus::Running => theme().primary_foreground,
            TaskStatus::Failed => theme()
                .color("destructive-foreground")
                .expect("destructive foreground token"),
            TaskStatus::Pending => theme().secondary_foreground,
            TaskStatus::Completed => theme().color("success").expect("success token"),
            TaskStatus::Stopped => theme().color("warning").expect("warning token"),
        })
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn task_progress(percent: f64) -> nsh_base::ProgressElement {
    nsh_base::progress()
        .value(percent.clamp(0.0, 100.0) as f32)
        .track_style(|style| {
            style
                .h_2()
                .w_full()
                .rounded_full()
                .overflow_hidden()
                .bg(move || theme().primary.opacity(0.2))
        })
        .indicator_style(|style| style.h_full().bg(move || theme().primary))
}
