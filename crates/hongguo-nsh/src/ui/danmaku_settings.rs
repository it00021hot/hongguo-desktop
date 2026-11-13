use crate::i18n::tr;
use nsh_base::{SliderValue, theme};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct DanmakuSettingsState {
    pub open: Signal<bool>,
    pub opacity: Signal<f32>,
    pub font_scale: Signal<f32>,
    pub density: Signal<f32>,
    pub area: Signal<f32>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum DanmakuField {
    Opacity,
    FontScale,
    Density,
    Area,
}

impl DanmakuField {
    pub const ALL: [Self; 4] = [Self::Opacity, Self::FontScale, Self::Density, Self::Area];
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Opacity => (0.1, 1.0),
            Self::FontScale => (0.5, 2.0),
            Self::Density => (0.0, 1.0),
            Self::Area => (0.25, 1.0),
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Opacity => "hongguo.danmakuOpacity",
            Self::FontScale => "hongguo.danmakuFontScale",
            Self::Density => "hongguo.danmakuDensity",
            Self::Area => "hongguo.danmakuArea",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Opacity => "player.danmakuOpacity",
            Self::FontScale => "player.danmakuFontSize",
            Self::Density => "player.danmakuDensity",
            Self::Area => "player.danmakuArea",
        }
    }
    pub fn default_value(self) -> f32 {
        if matches!(self, Self::Area) {
            0.25
        } else {
            0.5
        }
    }
    pub fn value(self, state: DanmakuSettingsState) -> Signal<f32> {
        match self {
            Self::Opacity => state.opacity,
            Self::FontScale => state.font_scale,
            Self::Density => state.density,
            Self::Area => state.area,
        }
    }
}

impl DanmakuSettingsState {
    /// In-memory fixture state; persistence belongs to the session adapter.
    pub fn new() -> Self {
        Self {
            open: signal(false),
            opacity: signal(0.5),
            font_scale: signal(0.5),
            density: signal(0.5),
            area: signal(0.25),
        }
    }
}

pub(crate) fn danmaku_settings(state: DanmakuSettingsState) -> View {
    settings_layout(state, 16.0)
}

pub(crate) fn danmaku_settings_compact(state: DanmakuSettingsState) -> View {
    settings_layout(state, 12.0)
}

fn settings_layout(state: DanmakuSettingsState, gap: f32) -> View {
    view()
        .gap(px(gap))
        .children(DanmakuField::ALL.map(move |field| {
            let value = field.value(state);
            let (min, max) = field.range();
            view()
                .gap_1p5()
                .child(
                    view()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .child(
                            text(move || tr(field.label()))
                                .text_xs()
                                .line_height(px(16.0))
                                .text_color(move || {
                                    theme()
                                        .color("video-controls")
                                        .expect("video controls")
                                        .opacity(0.9)
                                }),
                        )
                        .child(
                            text(move || format!("{}%", (value.get() * 100.0).round() as u32))
                                .text_xs()
                                .line_height(px(16.0))
                                .font_family("monospace")
                                .text_color(move || {
                                    theme()
                                        .color("video-controls")
                                        .expect("video controls")
                                        .opacity(0.7)
                                }),
                        ),
                )
                .child(
                    view()
                        .test_id(field.key())
                        .pointer_events(PointerEvents::BoxNone)
                        .child(
                            nsh_base::slider()
                                .accessibility_label(move || Some(tr(field.label())))
                                .min(f64::from(min))
                                .max(f64::from(max))
                                .step(0.05)
                                .value(move || Some(SliderValue::from(f64::from(value.get()))))
                                .on_change(move |event: &SliderValue| {
                                    let next = event.start() as f32;
                                    if next.is_finite() {
                                        value.set(next.clamp(min, max));
                                    }
                                })
                                .h(move || px(if is_desktop() { 4.0 } else { 48.0 }))
                                .track_style(|style| {
                                    style
                                        .h(move || px(if is_desktop() { 4.0 } else { 48.0 }))
                                        .justify_center()
                                })
                                .indicator_style(|style| {
                                    style.h_1().rounded_full().bg(move || {
                                        theme()
                                            .color("video-controls")
                                            .expect("controls")
                                            .opacity(0.35)
                                    })
                                })
                                .range_style(|style| {
                                    style.h_1().rounded_full().bg(move || {
                                        theme().color("video-controls").expect("controls")
                                    })
                                })
                                .thumb_style(|style| {
                                    style.size_3().rounded_full().bg(move || {
                                        theme().color("video-controls").expect("controls")
                                    })
                                })
                                .thumb_class("focus:outline-2 focus:outline-ring"),
                        ),
                )
        }))
}
