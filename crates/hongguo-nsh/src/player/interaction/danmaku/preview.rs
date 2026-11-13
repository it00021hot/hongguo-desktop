//! Same production flight renderer with a fixed local stream, without storage or APIs.
use super::{DanmakuPreferences, Handle, flight_model::admitted, model::Timeline};
use crate::ui;
use hongguo_api::danmaku::Danmaku;
use nsh_core::prelude::*;

pub(in crate::player) fn layer(
    display: ui::DanmakuSettingsState,
    enabled: Signal<bool>,
    position: Signal<(f64, f64)>,
    paused: Signal<bool>,
    ready: Signal<bool>,
) -> View {
    let mut timeline = Timeline::new("preview-series".into(), "preview-episode".into());
    let id = (0..100)
        .map(|index| format!("preview-flight-{index}"))
        .find(|id| admitted(id, 0.5))
        .expect("fixed sampled preview id");
    timeline.insert(Danmaku {
        comment_id: id,
        text: "墙钟弹幕[微笑]".into(),
        offset_ms: 42_000,
        digg_count: 0,
    });
    let handle = Handle {
        enabled,
        timeline: signal(timeline),
        position,
        motion: super::Motion::new(
            computed(|| true),
            paused,
            computed(|| false),
            signal(false),
            signal(None),
        ),
        preferences: DanmakuPreferences {
            enabled,
            display,
            error: signal(SharedString::default()),
        },
        error: signal(SharedString::default()),
        retry: signal(0),
    };
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::None)
        .on_layout(move |frame: &Bounds<Points>| {
            if frame.size.width.0 > 0.0 && frame.size.height.0 > 0.0 {
                ready.set(true);
            }
        })
        .child(overlay(handle))
}

fn overlay(handle: Handle) -> View {
    super::overlay::build(handle).test_id("danmaku-motion-preview")
}
