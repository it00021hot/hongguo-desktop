use super::{Callbacks, State};
use crate::{i18n::tr, player::enhancement::Controller};
use hongguo_nsh_video_api::{
    COMPONENT_NAME, EVENT_NAME, STATE_PROP, Source, VideoEvent, VideoProps,
};
use nsh_core::prelude::*;

pub(super) fn mount(state: State, callbacks: Callbacks) -> View {
    state.control.pixel.set(true);
    let callbacks = stored(std::rc::Rc::new(callbacks));
    let enhancement = Controller::current();
    effect(move || {
        if let Some(enhancement) = enhancement {
            let settings = enhancement.settings.get();
            if !state.active.get() || (!settings.anime4k_on && !settings.sharpen_on) {
                batch(|| {
                    enhancement
                        .phase
                        .set(hongguo_nsh_video_api::EnhancementPhase::Idle);
                    enhancement.message.set(SharedString::default());
                });
            }
        }
    });
    let duration = signal(None::<u64>);
    let source_id = state.control.source;
    let props = computed(move || {
        let mut source = Source::default();
        source.id = source_id.get();
        source.url = state.url.clone();
        source.frame_source = state.frame_source.clone();
        let mut props = VideoProps::default();
        props.source = Some(source);
        props.paused = state.paused.get();
        props.muted = state.muted.get();
        props.volume = state.volume.get();
        props.rate = f64::from(state.rate.get());
        props.progress_interval_ms = state.interval.get().round() as u32;
        props.active = state.active.get();
        props.resize_mode = state.resize_mode.get();
        if let Some(enhancement) = enhancement {
            props.enhancement = enhancement.settings.get();
        }
        props
    });
    view().flex_1().child(
        native(COMPONENT_NAME)
            .flex_1()
            .element_ref(&state.control.reference)
            .opacity(state.opacity)
            .prop(STATE_PROP, move || {
                serde_json::to_value(props.get()).expect("typed pixel video props")
            })
            .on_native_event(move |event: &nsh_core::element::NativeEvent| {
                if event.name.as_ref() != EVENT_NAME {
                    return;
                }
                let event = match serde_json::from_value::<VideoEvent>(event.data.clone()) {
                    Ok(event) => event,
                    Err(error) => {
                        log::error!("[NSH pixel video] invalid event: {error}");
                        (callbacks.get().error)(&tr("nsh.playbackFailed"));
                        return;
                    }
                };
                let id = match &event {
                    VideoEvent::LoadStart { source_id }
                    | VideoEvent::Load { source_id, .. }
                    | VideoEvent::Progress { source_id, .. }
                    | VideoEvent::Buffer { source_id, .. }
                    | VideoEvent::End { source_id }
                    | VideoEvent::Seeked { source_id, .. }
                    | VideoEvent::Error { source_id, .. }
                    | VideoEvent::EnhancementStatus { source_id, .. } => source_id,
                };
                if *id != source_id.get() {
                    return;
                }
                let callbacks = callbacks.get();
                match event {
                    VideoEvent::LoadStart { .. } => (callbacks.buffer)(&true),
                    VideoEvent::Load {
                        duration_ms,
                        width,
                        height,
                        ..
                    } => {
                        duration.set(duration_ms);
                        (callbacks.load)(&VideoInfo {
                            duration: duration_ms.unwrap_or(0) as f32 / 1000.0,
                            size: Size::new(
                                DevicePixels(width as i32),
                                DevicePixels(height as i32),
                            ),
                        });
                    }
                    VideoEvent::Progress {
                        position_ms,
                        duration_ms,
                        ..
                    } => {
                        duration.set(duration_ms);
                        if state.control.pending.get_untracked().is_none() {
                            (callbacks.progress)(&VideoProgress {
                                current_time: position_ms as f32 / 1000.0,
                                duration: duration_ms.unwrap_or(0) as f32 / 1000.0,
                            });
                        }
                    }
                    VideoEvent::Buffer { is_buffering, .. } => (callbacks.buffer)(&is_buffering),
                    VideoEvent::End { .. } => (callbacks.end)(),
                    VideoEvent::Seeked {
                        request_id,
                        position_ms,
                        ..
                    } => {
                        if state.control.pending.get_untracked().as_ref() != Some(&request_id) {
                            return;
                        }
                        state.control.pending.set(None);
                        (callbacks.progress)(&VideoProgress {
                            current_time: position_ms as f32 / 1000.0,
                            duration: duration.get_untracked().unwrap_or(0) as f32 / 1000.0,
                        });
                    }
                    VideoEvent::Error { code, detail, .. } => {
                        log::error!("[NSH pixel video] {code:?}: {detail}");
                        state.control.pending.set(None);
                        (callbacks.error)(&tr("nsh.playbackFailed"));
                    }
                    VideoEvent::EnhancementStatus {
                        phase,
                        settings,
                        code,
                        detail,
                        ..
                    } => {
                        if let Some(enhancement) = enhancement {
                            let current = enhancement.settings.get_untracked();
                            if !state.active.get_untracked()
                                || (!current.anime4k_on && !current.sharpen_on)
                                || current != settings
                            {
                                return;
                            }
                            enhancement.phase.set(phase);
                            if let Some(code) = code {
                                log::warn!("[NSH enhancement] {code:?}: {detail}");
                            }
                            let key = match phase {
                                hongguo_nsh_video_api::EnhancementPhase::Idle => None,
                                hongguo_nsh_video_api::EnhancementPhase::Loading => {
                                    Some("player.enhancePreparing")
                                }
                                hongguo_nsh_video_api::EnhancementPhase::Ready => {
                                    Some(if enhancement.settings.get_untracked().anime4k_on {
                                        "player.enhanceAnime4kOn"
                                    } else {
                                        "player.enhanceSharpenOn"
                                    })
                                }
                                hongguo_nsh_video_api::EnhancementPhase::Error => {
                                    Some(match code {
                                        Some(hongguo_nsh_video_api::ErrorCode::InputTooLarge) => {
                                            "player.enhanceErrTooLarge"
                                        }
                                        Some(hongguo_nsh_video_api::ErrorCode::EnhancementSlow) => {
                                            "player.enhanceErrSlow"
                                        }
                                        Some(hongguo_nsh_video_api::ErrorCode::GpuUnavailable) => {
                                            "player.enhanceErrGpuUnsupported"
                                        }
                                        _ => "player.enhanceErrGeneric",
                                    })
                                }
                            };
                            enhancement
                                .message
                                .set(key.map_or_else(SharedString::default, tr));
                        }
                    }
                }
            }),
    )
}
