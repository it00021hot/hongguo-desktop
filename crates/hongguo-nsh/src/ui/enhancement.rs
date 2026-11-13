use crate::i18n::tr;
use hongguo_nsh_video_api::{Enhancement, EnhancementPhase};
use nsh_base::{SliderValue, theme};
use nsh_core::prelude::*;

const STRENGTH_MAX: f64 = 100.0;

pub(crate) fn enhancement_settings(
    settings: Signal<Enhancement>,
    phase: Signal<EnhancementPhase>,
    message: Signal<SharedString>,
) -> View {
    view()
        .test_id("enhancement-settings")
        .gap_4()
        .child(
            view()
                .flex_row()
                .gap_1()
                .child(
                    toggle(
                        "player.enhanceAnime4k",
                        computed(move || settings.get().anime4k_on),
                    )
                    .on_press(move || {
                        settings.update(|value| value.anime4k_on = !value.anime4k_on)
                    }),
                )
                .child(
                    toggle(
                        "player.enhanceSharpen",
                        computed(move || settings.get().sharpen_on),
                    )
                    .on_press(move || {
                        settings.update(|value| value.sharpen_on = !value.sharpen_on)
                    }),
                ),
        )
        .child(when(
            move || settings.get().sharpen_on,
            move || {
                view()
                    .gap_1p5()
                    .child(
                        view()
                            .flex_row()
                            .justify_between()
                            .child(text(move || tr("player.enhanceStrength")).text_xs())
                            .child(
                                text(move || format!("{}%", settings.get().strength))
                                    .text_xs()
                                    .font_family("monospace"),
                            ),
                    )
                    .child(
                        nsh_base::slider()
                            .accessibility_label(move || Some(tr("player.enhanceStrength")))
                            .min(0.0)
                            .max(STRENGTH_MAX)
                            .step(1.0)
                            .value(move || {
                                Some(SliderValue::from(f64::from(settings.get().strength)))
                            })
                            .on_change(move |value: &SliderValue| {
                                let value = value.start();
                                if value.is_finite() {
                                    settings.update(|settings| {
                                        settings.strength =
                                            value.clamp(0.0, STRENGTH_MAX).round() as u8
                                    });
                                }
                            })
                            .h(move || px(if is_desktop() { 4.0 } else { 48.0 }))
                            .track_style(|style| {
                                style
                                    .h(move || px(if is_desktop() { 4.0 } else { 48.0 }))
                                    .justify_center()
                            })
                            .indicator_style(|style| {
                                style.h_1().rounded_full().bg(move || ink().opacity(0.35))
                            })
                            .range_style(|style| style.h_1().rounded_full().bg(ink))
                            .thumb_style(|style| style.size_3().rounded_full().bg(ink))
                            .thumb_class("focus:outline-2 focus:outline-ring"),
                    )
            },
        ))
        .child(note("player.enhanceNote"))
        .child(when(
            move || settings.get().anime4k_on,
            || note("player.enhanceAnime4kNote"),
        ))
        .child(when(
            move || !message.get().is_empty(),
            move || {
                text(message)
                    .text_size(px(11.0))
                    .line_height(px(18.0))
                    .text_color(move || {
                        ink().opacity(if phase.get() == EnhancementPhase::Error {
                            1.0
                        } else {
                            0.6
                        })
                    })
            },
        ))
}

fn toggle(label: &'static str, selected: Memo<bool>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .flex_1()
        .px_2()
        .py_1p5()
        .rounded_lg()
        .min_h(move || px(if is_desktop() { 28.0 } else { 48.0 }))
        .accessibility_label(move || Some(tr(label)))
        .selected(selected)
        .bg(move || {
            if selected.get() {
                ink()
            } else {
                ink().opacity(0.1)
            }
        })
        .text_color(move || {
            if selected.get() {
                theme().color("video-surface").expect("video surface")
            } else {
                ink().opacity(0.8)
            }
        })
        .hover(move |style| {
            style.bg(move || {
                if selected.get() {
                    ink()
                } else {
                    ink().opacity(0.2)
                }
            })
        })
        .active(|style| style.opacity(0.8))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .child(text(move || tr(label)).text_xs())
}

fn ink() -> nsh_core::element::AdaptiveColor {
    theme().color("video-controls").expect("video controls")
}

fn note(label: &'static str) -> Text {
    text(move || tr(label))
        .text_size(px(11.0))
        .line_height(px(18.0))
        .text_color(move || ink().opacity(0.6))
}
