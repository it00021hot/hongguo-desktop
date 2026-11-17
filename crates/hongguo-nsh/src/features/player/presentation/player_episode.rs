use crate::ui::Icon;
use crate::ui::icon;
use crate::ui::motion::FAST_MS;
use crate::ui::palette;
use crate::ui::radii;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) const PICKER_GRID_GAP: f32 = 8.0;
pub(crate) const PICKER_DESKTOP_COLUMNS: u32 = 8;
const PICKER_TOUCH_SIZE: f32 = 48.0;

pub(crate) fn picker_mobile_columns(available_width: f32) -> u32 {
    ((available_width + PICKER_GRID_GAP) / (PICKER_TOUCH_SIZE + PICKER_GRID_GAP))
        .floor()
        .clamp(1.0, PICKER_DESKTOP_COLUMNS as f32) as u32
}

pub(crate) fn picker_tab(
    label: impl IntoReactive<SharedString>,
    selected: impl IntoReactive<bool>,
    count: impl IntoReactive<usize>,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    let selected = selected.into_reactive();
    let selected = computed(move || selected.get());
    let count = count.into_reactive();
    let count = computed(move || count.get());
    nsh_base::button()
        .accessibility_label(move || Some(label.get()))
        .selected(selected)
        .min_h(move || px(if is_desktop() { 0.0 } else { PICKER_TOUCH_SIZE }))
        .min_w(move || px(if is_desktop() { 0.0 } else { PICKER_TOUCH_SIZE }))
        .relative()
        .flex_row()
        .items_center()
        .gap_1()
        .pb_1p5()
        .text_color(move || theme().muted_foreground)
        .font_weight(FontWeight::SEMIBOLD)
        .selected_style(|style| style.text_color(move || theme().primary))
        .hover(|style| style.text_color(move || theme().foreground))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .child(text(move || label.get()).text_sm().line_height(px(20.0)))
        .child(when(
            move || count.get() > 0,
            move || {
                view()
                    .px_1p5()
                    .rounded_full()
                    .bg(palette::selection_background)
                    .text_color(palette::selection_foreground)
                    .child(
                        text(move || count.get().to_string())
                            .text_xs()
                            .line_height(px(16.0)),
                    )
            },
        ))
        .child(when(
            move || selected.get(),
            || {
                view()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h_0p5()
                    .rounded_full()
                    .bg(move || theme().primary)
            },
        ))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
}

pub(crate) fn picker_close(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    nsh_base::button()
        .accessibility_label(move || Some(label.get()))
        .size(px(if is_desktop() { 28.0 } else { 48.0 }))
        .items_center()
        .justify_center()
        .rounded(radii::control())
        .text_color(move || theme().muted_foreground)
        .hover(|style| {
            style
                .bg(move || theme().accent)
                .text_color(move || theme().accent_foreground)
        })
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .child(icon(Icon::Dropdown))
}

pub(crate) fn picker_downloaded(active: impl IntoReactive<bool>) -> View {
    let active = active.into_reactive();
    view()
        .absolute()
        .top_1()
        .right_1()
        .size_1p5()
        .rounded_full()
        .pointer_events(PointerEvents::None)
        .bg(move || {
            if active.get() {
                palette::selection_foreground()
            } else {
                theme().color("success").expect("success token")
            }
        })
}

pub(crate) fn picker_season(current: impl IntoReactive<bool>, unreleased: bool) -> View {
    let current = current.into_reactive();
    let current = computed(move || current.get());
    view()
        .flex_row()
        .items_center()
        .gap_2p5()
        .p_2p5()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || {
            palette::selection_background().opacity(if current.get() { 0.8 } else { 0.0 })
        })
        .bg(move || {
            if current.get() {
                palette::selection_background()
            } else {
                theme().secondary
            }
        })
        .text_color(move || {
            if current.get() {
                palette::selection_foreground()
            } else {
                theme().secondary_foreground
            }
        })
        .hover(move |style| {
            style.bg(move || {
                if current.get() {
                    palette::selection_background()
                } else {
                    if unreleased {
                        theme().secondary
                    } else {
                        theme().accent
                    }
                }
            })
        })
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
}

pub(crate) fn picker_reserve(
    label: impl IntoReactive<SharedString>,
    reserved: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let reserved = reserved.into_reactive();
    let reserved = computed(move || reserved.get());
    nsh_base::button()
        .h(px(if is_desktop() { 28.0 } else { 48.0 }))
        .px_3()
        .flex_row()
        .items_center()
        .gap_1()
        .rounded_full()
        .flex_shrink_0()
        .bg(move || {
            if reserved.get() {
                theme().accent
            } else {
                palette::selection_background()
            }
        })
        .text_color(move || {
            if !reserved.get() {
                palette::selection_foreground()
            } else {
                theme().accent_foreground
            }
        })
        .hover(move |style| style.opacity(if reserved.get() { 1.0 } else { 0.9 }))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .disabled_style(move |style| style.opacity(if reserved.get() { 1.0 } else { 0.6 }))
        .child(icon(Icon::Reservations).size_3())
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn picker_play(label: impl IntoReactive<SharedString>) -> View {
    view()
        .h(px(28.0))
        .px_3()
        .flex_row()
        .items_center()
        .gap_1()
        .rounded_full()
        .flex_shrink_0()
        .border_1()
        .border_color(move || theme().border)
        .text_color(move || theme().foreground)
        .child(icon(Icon::Play).size_3())
        .child(text(label).text_xs().line_height(px(16.0)))
}
