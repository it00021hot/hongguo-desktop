//! Source-owned native playback events; every callback verifies the active lease identity.
use super::{buffering::Buffering, completion, lifecycle, prepare, progress, transport};
use crate::{bridge, pages::SessionHandle};
use nsh_core::prelude::*;

#[derive(Clone)]
struct VideoSource {
    url: String,
    frame_source: Option<String>,
    resume_at: f64,
    history: hongguo_common::model::LocalHistoryItem,
    expected_duration: Option<f64>,
}

impl PartialEq for VideoSource {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
    }
}

#[derive(Clone, Copy)]
pub(super) struct State {
    pub session: SessionHandle,
    pub source: Resource<prepare::Playback, bridge::Failure>,
    pub visible: Memo<bool>,
    pub play_allowed: Memo<bool>,
    pub paused: Signal<bool>,
    pub rate: Signal<f32>,
    pub volume: Signal<f32>,
    pub muted: Signal<bool>,
    pub position: Signal<(f64, f64)>,
    pub pending_seek: Signal<Option<f64>>,
    pub buffering: Signal<bool>,
    pub failure: Signal<SharedString>,
    pub notice: Signal<SharedString>,
    pub gestures: super::navigation::Controller,
    pub chrome: super::chrome::Chrome,
    pub video_size: Signal<(f64, f64)>,
    pub comments_open: Signal<bool>,
    pub download_open: Signal<bool>,
}

pub(super) fn mount<E: IntoElement>(
    state: State,
    overlay: impl Fn() -> E + 'static,
    next: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let State {
        session,
        source,
        visible,
        play_allowed,
        paused,
        rate,
        volume,
        muted,
        position,
        pending_seek,
        buffering,
        failure,
        notice,
        gestures,
        chrome,
        video_size,
        comments_open,
        download_open,
    } = state;
    each(
        move || {
            source.with(|source| {
                source
                    .map(|source| VideoSource {
                        url: source.lease.url().to_owned(),
                        frame_source: source.lease.frame_source(),
                        resume_at: source.resume_at,
                        history: source.history.clone(),
                        expected_duration: source.expected_duration,
                    })
                    .into_iter()
                    .collect::<Vec<_>>()
            })
        },
        |source: &VideoSource| source.url.clone(),
        move |mounted, _| {
            let mounted = mounted.get_untracked();
            let control = super::renderer::Control::new();
            let modes = crate::window_modes::WindowModes::current();
            let resize_mode = crate::ui::player_resize_mode(
                video_size,
                computed(move || {
                    modes.is_some_and(|modes| modes.state().mode == crate::window_modes::Mode::Mini)
                }),
            );
            if pending_seek.get_untracked().is_none() {
                pending_seek.set(transport::initial_seek(mounted.resume_at));
            }
            let loaded = signal(false);
            let initial_load = stored(transport::LoadLatch::default());
            let seek = stored(transport::SeekLatch::default());
            let native_position = signal((0.0_f64, 0.0_f64));
            let buffer_state = stored(Buffering::default());
            let completion = stored(completion::Gate::default());
            let source_url = stored(mounted.url.clone());
            let current_source = move || {
                transport::source_current(
                    source.loading(),
                    source.error().is_some(),
                    source.with(|source| {
                        source.is_some_and(|source| source.lease.url() == source_url.get())
                    }),
                )
            };
            let finished_series = stored(mounted.history.series_id.clone());
            let finished_episode = mounted.history.vid_index;
            let recorder = progress::observe(session, mounted.history, visible);
            effect(move || {
                if paused.get() || !play_allowed.get() {
                    recorder.flush();
                }
            });
            effect(move || {
                if pending_seek.get().is_none() {
                    seek.with(transport::SeekLatch::clear);
                    return;
                }
                if current_source()
                    && loaded.get()
                    && let Some(target) = pending_seek.get()
                {
                    buffering.set(true);
                    let native_duration = native_position.get().1;
                    if !transport::seek_available(target, native_duration) {
                        failure.set(crate::i18n::tr("player.seekUnavailable"));
                        paused.set(true);
                        pending_seek.set(None);
                        buffering.set(false);
                        return;
                    }
                    if seek.with(|seek| seek.dispatch(target)) {
                        control.seek(target);
                    }
                }
            });
            view()
                .flex_1()
                .relative()
                .pointer_events(PointerEvents::BoxOnly)
                .accessible(true)
                .role(nsh_core::AccessibilityRole::Button)
                .label(move || {
                    crate::i18n::tr(if paused.get() {
                        "player.playVideo"
                    } else {
                        "player.pauseVideo"
                    })
                })
                .on_press(move || {
                    if current_source() && play_allowed.get_untracked() {
                        chrome.wake();
                        paused.update(|paused| *paused = !*paused);
                    }
                })
                .child(super::renderer::mount(
                    super::renderer::State {
                        url: mounted.url,
                        frame_source: mounted.frame_source,
                        codec_extension: Some(std::rc::Rc::new(move || {
                            source.with(|source| {
                                source
                                    .ok_or_else(|| "media source is no longer current".to_string())?
                                    .lease
                                    .enable_codec_extension(
                                        pending_seek
                                            .get_untracked()
                                            .unwrap_or(native_position.get_untracked().0),
                                    )
                            })
                        })),
                        control,
                        rate: computed(move || rate.get()),
                        interval: computed(|| 250.0),
                        volume,
                        muted: computed(move || muted.get() || pending_seek.get().is_some()),
                        opacity: computed(|| 1.0_f32),
                        paused: computed(move || {
                            !play_allowed.get() || !current_source() || paused.get()
                        }),
                        active: computed(move || play_allowed.get() && current_source()),
                        resize_mode,
                    },
                    super::renderer::Callbacks {
                        load: Box::new(move |info: &VideoInfo| {
                            if !current_source() {
                                return;
                            }
                            let size =
                                (f64::from(info.size.width.0), f64::from(info.size.height.0));
                            video_size.set(size);
                            if let Some(modes) = crate::window_modes::WindowModes::current() {
                                modes.refit(crate::window_modes::portrait_aspect(size.0, size.1));
                            }
                            native_position.update(|position| position.1 = info.duration.into());
                            let duration =
                                effective_duration(mounted.expected_duration, info.duration.into());
                            if let Some(at) = initial_load.with(|load| {
                                load.seek(pending_seek.get_untracked(), mounted.resume_at, duration)
                            }) {
                                position.set((0.0, duration));
                                let target = transport::initial_seek(at);
                                pending_seek.set(target);
                                if target.is_none() {
                                    buffering
                                        .set(buffer_state.with(|state| state.is_buffering(false)));
                                }
                                loaded.set(true);
                            } else {
                                position.update(|position| position.1 = duration);
                            }
                            failure.set(SharedString::default());
                        }),
                        progress: Box::new(move |progress: &VideoProgress| {
                            if !progress.current_time.is_finite() || progress.current_time < 0.0 {
                                return;
                            }
                            if loaded.get_untracked() && current_source() {
                                let duration = effective_duration(
                                    mounted.expected_duration,
                                    progress.duration.into(),
                                );
                                let target = pending_seek.get_untracked();
                                let confirmed = target.is_some_and(|target| {
                                    seek.with(|seek| {
                                        seek.confirmed(target, progress.current_time.into())
                                    })
                                });
                                if confirmed {
                                    pending_seek.set(None);
                                }
                                let seeking = target.is_some() && !confirmed;
                                let previous = native_position.get_untracked();
                                native_position
                                    .set((progress.current_time.into(), progress.duration.into()));
                                if let Some(buffered) = buffer_state.with(|state| {
                                    state.progress(
                                        confirmed,
                                        f64::from(progress.current_time) > previous.0,
                                        seeking,
                                    )
                                }) {
                                    buffering.set(buffered);
                                }
                                if !seeking {
                                    position.set((progress.current_time.into(), duration));
                                    recorder.record(progress.current_time.into(), duration);
                                }
                            }
                        }),
                        buffer: Box::new(move |value: &bool| {
                            if current_source() {
                                buffering.set(buffer_state.with(|state| {
                                    state.native_event(
                                        *value,
                                        pending_seek.get_untracked().is_some(),
                                    )
                                }));
                            }
                        }),
                        error: Box::new(move |message: &str| {
                            if current_source() {
                                failure.set(message.to_owned().into());
                                pending_seek.set(None);
                                buffering.set(false);
                            }
                        }),
                        end: Box::new(move || {
                            if !current_source() {
                                return;
                            }
                            if completion::end_is_blocked(
                                paused.get_untracked(),
                                pending_seek.get_untracked().is_some(),
                            ) {
                                recorder.flush();
                                return;
                            }
                            let ended = completion.with(|gate| {
                                gate.on_end_for(
                                    loaded.get_untracked(),
                                    comments_open.get_untracked(),
                                )
                            });
                            match ended {
                                completion::End::Ignore => return,
                                completion::End::Repeat => {
                                    batch(|| {
                                        pending_seek.set(Some(0.0));
                                        paused.set(false);
                                    });
                                    return;
                                }
                                completion::End::Complete => {}
                            }
                            recorder.complete();
                            lifecycle::delete_finished(
                                session,
                                finished_series.get(),
                                finished_episode,
                                notice,
                            );
                            if play_allowed.get_untracked()
                                && !paused.get_untracked()
                                && !download_open.get_untracked()
                                && session
                                    .with(|session| session.state.settings().auto_next_episode)
                            {
                                gestures.enter_binge();
                                chrome.arm_quiet();
                                next();
                            } else {
                                paused.set(true);
                            }
                        }),
                    },
                ))
                .child(overlay())
                .map(move |surface| gestures.bind(surface))
        },
    )
}

fn effective_duration(expected: Option<f64>, native: f64) -> f64 {
    if native.is_finite() && native > 0.0 {
        native
    } else {
        expected.unwrap_or(0.0)
    }
}

#[cfg(test)]
mod duration_tests {
    #[test]
    fn actual_audio_duration_takes_precedence_over_rounded_episode_metadata() {
        assert_eq!(super::effective_duration(Some(168.0), 168.533), 168.533);
        assert_eq!(super::effective_duration(None, f64::NAN), 0.0);
    }
}
