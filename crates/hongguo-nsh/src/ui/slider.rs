use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) use nsh_base::SliderValue;

pub(crate) fn slider(label: impl IntoReactive<SharedString>) -> nsh_base::SliderElement {
    let label = label.into_reactive();
    nsh_base::slider()
        .accessibility_label(move || Some(label.get()))
        .thumb_class("hover:opacity-90 active:opacity-80 focus:outline-2 focus:outline-ring")
        .disabled_style(|style| style.opacity(0.45))
        .track_style(|style| {
            style
                .min_h(move || px(if is_desktop() { 16.0 } else { 48.0 }))
                .justify_center()
        })
        .indicator_style(|style| {
            style
                .h_2()
                .rounded_full()
                .border_1()
                .border_color(move || {
                    theme()
                        .color("native-range-border")
                        .expect("native range token")
                })
                .bg(move || {
                    theme()
                        .color("native-range-track")
                        .expect("native range token")
                })
        })
        .range_style(|style| {
            style.rounded_full().bg(move || {
                theme()
                    .color("native-range-fill")
                    .expect("native range token")
            })
        })
        .thumb_style(|style| {
            style.size(px(16.0)).rounded_full().bg(move || {
                theme()
                    .color("native-range-fill")
                    .expect("native range token")
            })
        })
}
