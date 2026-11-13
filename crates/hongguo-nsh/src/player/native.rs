//! Source-owned native playback events; every callback verifies the active lease identity.
use super::{
    buffering::Buffering, completion, lifecycle, prepare, progress, seek_confirmation, seek_ready,
    transport,
};
use crate::{bridge, pages::SessionHandle};
use nsh_core::prelude::*;

#[derive(Clone)]
struct VideoSource {
    url: String,
    resume_at: f64,
    history: hongguo_common::model::LocalHistoryItem,
    expected_duration: Option<f64>,
    growing: bool,
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
                        resume_at: source.resume_at,
                        history: source.history.clone(),
                        expected_duration: source.expected_duration,
                        growing: source.growing,
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
            let exact_origin = mounted.growing && super::renderer::pixel_supported();
            if pending_seek.get_untracked().is_none() {
                pending_seek.set(transport::initial_seek(mounted.resume_at, exact_origin));
            }
            let loaded = signal(false);
            let initial_load = stored(transport::LoadLatch::default());
            let seek = stored(transport::SeekLatch::default());
            let issued_seek = signal(None::<f64>);
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
            let readiness = seek_ready::observe(
                pending_seek,
                move || current_source() && loaded.get(),
                move |target| {
                    let readiness = source.with(|source| {
                        source
                            .ok_or_else(|| "media source is unavailable".to_owned())?
                            .lease
                            .readiness()
                    });
                    async move {
                        if let Some(readiness) = readiness? {
                            readiness.wait_for_available(target).await?;
                        }
                        Ok(())
                    }
                },
            );
            let confirming = seek_confirmation::observe(
                issued_seek,
                pending_seek,
                paused,
                move || {
                    crate::media::paused_seek_needs_progress_drive()
                        && !super::renderer::pixel_supported()
                        && current_source()
                        && play_allowed.get()
                },
                move || {
                    failure.set(crate::i18n::tr("player.seekConfirmationFailed"));
                    issued_seek.set(None);
                    pending_seek.set(None);
                    buffering.set(false);
                },
            );
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
                    issued_seek.set(None);
                    return;
                }
                if current_source()
                    && loaded.get()
                    && let Some(target) = pending_seek.get()
                {
                    buffering.set(true);
                    if readiness.loading() {
                        return;
                    }
                    if let Some(error) = readiness.error() {
                        failure.set(error.into());
                        paused.set(true);
                        pending_seek.set(None);
                        buffering.set(false);
                        return;
                    }
                    if !readiness.with(|ready| ready.is_some_and(|ready| *ready == target)) {
                        return;
                    }
                    let native_duration = native_position.get().1;
                    let availability = source.with(|source| {
                        source.map(|source| {
                            let published = source.lease.available_duration()?;
                            Ok::<_, String>((published, source.lease.is_complete()?))
                        })
                    });
                    let Some(availability) = availability else {
                        return;
                    };
                    let (published, complete) = match availability {
                        Ok(availability) => availability,
                        Err(error) => {
                            failure.set(error.into());
                            paused.set(true);
                            pending_seek.set(None);
                            buffering.set(false);
                            return;
                        }
                    };
                    if transport::seek_exceeds_complete(target, native_duration, complete) {
                        failure.set(crate::i18n::tr("player.seekUnavailable"));
                        paused.set(true);
                        pending_seek.set(None);
                        buffering.set(false);
                        return;
                    }
                    if transport::seek_available(
                        target,
                        native_duration,
                        published,
                        mounted.growing,
                    ) && seek.with(|seek| seek.dispatch(target))
                    {
                        control.seek(target);
                        issued_seek.set(Some(target));
                    }
                }
            });
            view()
                .flex_1()
                .relative()
                .pointer_events(PointerEvents::BoxOnly)
                .accessible(true)
                .role(nsh_core::AccessibilityRole::Button)
                .label(move || crate::i18n::tr(if paused.get() { "player.playVideo" } else { "player.pauseVideo" }))
                .on_press(move || {
                    if current_source() && play_allowed.get_untracked() {
                        chrome.wake();
                        paused.update(|paused| *paused = !*paused);
                    }
                })
                .child(
                    super::renderer::mount(super::renderer::State {
                        url: mounted.url,
                        control,
                        rate: computed(move || if confirming.get() { seek_confirmation::RATE } else { rate.get() }),
                        interval: computed(move || if confirming.get() { seek_confirmation::PROGRESS_INTERVAL } else { seek_confirmation::NORMAL_PROGRESS_INTERVAL }),
                        volume,
                        muted: computed(move || muted.get() || pending_seek.get().is_some()),
                        opacity: computed(move || if pending_seek.get().is_some() { 0.0_f32 } else { 1.0_f32 }),
                        paused: computed(move || !play_allowed.get() || !current_source() || (paused.get() && !confirming.get())),
                        active: computed(move || play_allowed.get() && current_source()),
                        resize_mode,
                    }, super::renderer::Callbacks {
                        load: Box::new(move |info: &VideoInfo| {
                            if !current_source() {
                                return;
                            }
                            let size = (f64::from(info.size.width.0), f64::from(info.size.height.0));
                            video_size.set(size);
                            if let Some(modes) = crate::window_modes::WindowModes::current() {
                                modes.refit(crate::window_modes::portrait_aspect(size.0, size.1));
                            }
                            native_position.update(|position| position.1 = info.duration.into());
                            let duration = effective_duration(
                                mounted.expected_duration,
                                info.duration.into(),
                                mounted.growing,
                            );
                            if let Some(at) = initial_load.with(|load| load.seek(pending_seek.get_untracked(), mounted.resume_at, duration)) {
                                position.set((0.0, duration));
                                let target = transport::initial_seek(at, exact_origin);
                                pending_seek.set(target);
                                if target.is_none() { buffering.set(buffer_state.with(|state| state.is_buffering(false))); }
                                loaded.set(true);
                            } else {
                                position.update(|position| position.1 = duration);
                            }
                            failure.set(SharedString::default());
                        }),
                        progress: Box::new(move |progress: &VideoProgress| {
                            if !progress.current_time.is_finite() || progress.current_time < 0.0 { return; }
                            if loaded.get_untracked() && current_source() {
                                let duration = effective_duration(
                                    mounted.expected_duration,
                                    progress.duration.into(),
                                    mounted.growing,
                                );
                                let target = pending_seek.get_untracked();
                                let confirmed = target.is_some_and(|target| seek.with(|seek| seek.confirmed(target, progress.current_time.into())));
                                if confirmed { pending_seek.set(None); }
                                let seeking = target.is_some() && !confirmed;
                                let previous = native_position.get_untracked();
                                native_position.set((progress.current_time.into(), progress.duration.into()));
                                if let Some(buffered) = buffer_state.with(|state| state.progress(
                                    confirmed, f64::from(progress.current_time) > previous.0, seeking,
                                )) {
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
                                buffering.set(buffer_state.with(|state| state.native_event(*value, pending_seek.get_untracked().is_some())));
                            }
                        }),
                        error: Box::new(move |message: &str| {
                            if current_source() {
                                failure.set(message.to_owned().into());
                                issued_seek.set(None);
                                pending_seek.set(None);
                                buffering.set(false);
                            }
                        }),
                        end: Box::new(move || {
                            if !current_source() {
                                return;
                            }
                            if completion::end_is_blocked(paused.get_untracked(), pending_seek.get_untracked().is_some(), confirming.get_untracked()) {
                                recorder.flush();
                                return;
                            }
                            let producer_complete = source.with(|source| source.map(|source| source.lease.is_complete()));
                            let Some(producer_complete) = producer_complete else { return; };
                            let producer_complete = match producer_complete {
                                Ok(complete) => complete,
                                Err(error) => { failure.set(error.into()); recorder.flush(); return; }
                            };
                            let native = native_position.get_untracked();
                            let published_duration = if mounted.growing && producer_complete {
                                match source.with(|ready| ready.map(|ready| ready.lease.available_duration())) {
                                    Some(Ok(duration)) => duration,
                                    Some(Err(error)) => { failure.set(error.into()); recorder.flush(); return; }
                                    None => return,
                                }
                            } else { None };
                            let ended = completion.with(|gate| gate.on_end_for(completion::Evidence {
                                loaded: loaded.get_untracked(), growing: mounted.growing, producer_complete,
                                position: native.0, native_duration: native.1, expected_duration: mounted.expected_duration, published_duration,
                            }, comments_open.get_untracked()));
                            match ended {
                                completion::End::Ignore => return,
                                completion::End::Repeat => {
                                    batch(|| {
                                        pending_seek.set(Some(0.0));
                                        paused.set(false);
                                    });
                                    return;
                                }
                                completion::End::AwaitMore => {
                                    if buffer_state.with(Buffering::defer_end) {
                                        log::warn!("[NSH playback] deferred incomplete end for {}/{}: at={} native={} expected={:?} producer_complete={}", finished_series.get(), finished_episode, native.0, native.1, mounted.expected_duration, producer_complete);
                                    }
                                    buffering.set(true); recorder.flush(); return;
                                }
                                completion::End::Complete => {}
                            }
                            recorder.complete();
                            lifecycle::delete_finished(session, finished_series.get(), finished_episode, notice);
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
                    }),
                )
                .child(overlay())
                .map(move |surface| gestures.bind(surface))
        },
    )
}

fn effective_duration(expected: Option<f64>, native: f64, growing: bool) -> f64 {
    if growing && let Some(expected) = expected {
        return expected;
    }
    if native.is_finite() && native > 0.0 {
        native
    } else {
        expected.unwrap_or(0.0)
    }
}

#[cfg(test)]
mod duration_tests {
    #[test]
    fn a_growing_first_fragment_does_not_shrink_the_episode_to_its_prefix() {
        assert_eq!(super::effective_duration(Some(168.0), 0.2667, true), 168.0);
        assert_eq!(
            super::effective_duration(Some(168.0), 168.533, false),
            168.533
        );
        assert_eq!(super::effective_duration(None, f64::NAN, false), 0.0);
    }
}
