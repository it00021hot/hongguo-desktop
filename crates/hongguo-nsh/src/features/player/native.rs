//! Source-owned native playback events; every callback verifies the active lease identity.
use super::{buffering::Buffering, completion, lifecycle, prepare, progress, transport};
use crate::{bridge, features::SessionHandle, media::MediaLease};
use nsh_core::prelude::*;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct VideoSource {
    url: String,
    frame_source: Option<String>,
    resume_at: f64,
    history: hongguo_common::model::LocalHistoryItem,
    expected_duration: Option<f64>,
    /// Keeps the loopback source registered until this layer leaves `shown`/`incoming`.
    _lease: Arc<MediaLease>,
}

impl PartialEq for VideoSource {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
    }
}

impl VideoSource {
    pub(super) fn can_overlap_picture(&self, enhancement: bool) -> bool {
        transport::overlap_picture(
            cfg!(target_os = "windows"),
            self.frame_source.is_some(),
            enhancement,
        )
    }

    pub(super) fn from_playback(source: &prepare::Playback) -> Self {
        Self {
            url: source.lease.url().to_owned(),
            frame_source: source.lease.frame_source(),
            resume_at: source.resume_at,
            history: source.history.clone(),
            expected_duration: source.expected_duration,
            _lease: Arc::clone(&source.lease),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct State {
    pub session: SessionHandle,
    pub source: Resource<prepare::Playback, bridge::Failure>,
    pub routed: Memo<bool>,
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
    pub relay_notice: Signal<SharedString>,
    pub gestures: super::navigation::Controller,
    pub chrome: super::chrome::Chrome,
    pub video_size: Signal<(f64, f64)>,
    pub comments_open: Signal<bool>,
    pub download_open: Signal<bool>,
    pub has_surface: Signal<bool>,
    pub staged: Signal<Option<VideoSource>>,
}

pub(super) fn mount<E: IntoElement>(
    state: State,
    overlay: impl Fn() -> E + 'static,
    next: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let State {
        session,
        source,
        routed,
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
        relay_notice,
        gestures,
        chrome,
        video_size,
        comments_open,
        download_open,
        has_surface,
        staged,
    } = state;
    let enhancement = super::enhancement::Controller::current();
    let shown = signal(None::<VideoSource>);
    let incoming = signal(None::<VideoSource>);
    let incoming_ready = signal(false);
    let incoming_opaque = signal(false);
    effect(move || {
        has_surface.set(shown.get().is_some() || incoming.get().is_some());
    });
    effect(move || {
        incoming.get();
        incoming_ready.set(false);
        incoming_opaque.set(false);
    });
    effect(move || {
        if source.state() == ResourceState::Errored {
            incoming.set(None);
            staged.set(None);
            chrome.end_handoff();
            return;
        }
        let resource = source.with(|source| source.map(VideoSource::from_playback));
        if resource.is_none()
            && transport::drop_shown_without_resource(source.loading(), chrome.silent_handoff())
        {
            incoming.set(None);
            shown.set(None);
            staged.set(None);
            return;
        }
        let shown_now = shown.get_untracked();
        let shown_url = shown_now.as_ref().map(|shown| shown.url.clone());
        let staged_now = staged.get();
        let resource_url = resource.as_ref().map(|resource| resource.url.clone());
        let candidate = transport::incoming_candidate(
            shown_url.as_deref(),
            staged_now.as_ref().map(|staged| staged.url.as_str()),
            resource_url.as_deref(),
        )
        .map(str::to_owned);
        let next = match candidate.as_deref() {
            Some(url) if staged_now.as_ref().is_some_and(|staged| staged.url == url) => staged_now,
            Some(url) => resource.filter(|resource| resource.url == url),
            None => None,
        };
        if let Some(next) = next {
            let enhanced = enhancement.is_some_and(|controller| {
                let settings = controller.settings.get();
                settings.anime4k_on || settings.sharpen_on
            });
            let overlap = next.can_overlap_picture(enhanced);
            if shown_now.is_some() && overlap {
                if incoming
                    .get_untracked()
                    .as_ref()
                    .is_none_or(|incoming| incoming.url != next.url)
                {
                    incoming.set(Some(next));
                }
            } else if resource_url.as_deref() == Some(next.url.as_str()) {
                // Preparation may finish before a switch, but Windows needs
                // a visible custom surface before it can emit its first frame.
                shown.set(Some(next));
                incoming.set(None);
                chrome.end_handoff();
            }
            return;
        }
        if incoming.get_untracked().is_some()
            && resource_url
                .as_deref()
                .is_some_and(|resource_url| shown_url.as_deref() == Some(resource_url))
        {
            incoming.set(None);
            chrome.end_handoff();
        }
    });
    effect(move || {
        let Some(next) = incoming.get() else {
            return;
        };
        let resource_url = source.with(|source| source.map(|source| source.lease.url().to_owned()));
        if !transport::can_finish_handoff(
            &next.url,
            resource_url.as_deref(),
            incoming_ready.get(),
            incoming_opaque.get(),
        ) {
            return;
        }
        let url = next.url.clone();
        shown.set(Some(next));
        incoming.set(None);
        if staged
            .get_untracked()
            .as_ref()
            .is_some_and(|staged| staged.url == url)
        {
            staged.set(None);
        }
        chrome.end_handoff();
    });
    effect(move || {
        let rate = rate.get();
        source.with(|source| {
            if let Some(source) = source
                && let Err(error) = source.lease.set_playback_rate(rate)
            {
                log::error!("[NSH media] could not update buffering rate: {error}");
            }
        });
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
            let showing = shown
                .with(|shown| shown.as_ref().map(|shown| shown.url.clone()))
                .or_else(|| incoming.with(|incoming| incoming.as_ref().map(|s| s.url.clone())));
            let current = source.with(|source| {
                source.is_some_and(|source| {
                    showing
                        .as_ref()
                        .is_some_and(|url| source.lease.url() == url)
                })
            });
            if !source.loading()
                && source.error().is_none()
                && current
                && play_allowed.get_untracked()
            {
                chrome.wake();
                paused.update(|paused| *paused = !*paused);
            }
        })
        .child(each(
            move || {
                let mut layers = Vec::new();
                if let Some(shown) = shown.get() {
                    layers.push(shown);
                }
                if let Some(incoming) = incoming.get()
                    && layers.first().is_none_or(|shown| shown.url != incoming.url)
                {
                    layers.push(incoming);
                }
                layers
            },
            |source: &VideoSource| source.url.clone(),
            move |mounted, _| {
                let mounted = mounted.get_untracked();
                let source_url = stored(mounted.url.clone());
                let is_incoming = computed(move || {
                    incoming.with(|incoming| {
                        incoming
                            .as_ref()
                            .is_some_and(|incoming| incoming.url == source_url.get())
                    })
                });
                let incoming_revealing = computed(move || {
                    is_incoming.get()
                        && incoming_ready.get()
                        && source.with(|source| {
                            source.is_some_and(|source| source.lease.url() == source_url.get())
                        })
                });
                let skip_entrance = is_incoming.get_untracked();
                let layer_motion = if skip_entrance {
                    let fade = motion();
                    fade.value(MotionChannel::Opacity).set(0.0);
                    fade
                } else {
                    motion()
                };
                if skip_entrance {
                    effect(move || {
                        if !incoming_revealing.get() {
                            return;
                        }
                        if incoming_opaque.get_untracked() {
                            return;
                        }
                        let expected_url = source_url.get();
                        let end = layer_motion.play(super::presentation::video::player_crossfade());
                        spawn(async move {
                            let _ = end.await;
                            if incoming.with_untracked(|incoming| {
                                incoming
                                    .as_ref()
                                    .is_some_and(|incoming| incoming.url == expected_url)
                            }) {
                                incoming_opaque.set(true);
                            }
                        });
                    });
                }
                let entrance = (!skip_entrance).then(|| {
                    super::presentation::video::player_source_motion(gestures.direction())
                });
                let control = super::renderer::Control::new();
                let modes = crate::window_modes::WindowModes::current();
                let resize_mode = crate::features::player::presentation::player_resize_mode(
                    video_size,
                    computed(move || {
                        modes.is_some_and(|modes| {
                            modes.state().mode == crate::window_modes::Mode::Mini
                        })
                    }),
                );
                if !is_incoming.get_untracked() && pending_seek.get_untracked().is_none() {
                    pending_seek.set(transport::initial_seek(mounted.resume_at));
                }
                let incoming_seek = stored(if is_incoming.get_untracked() {
                    transport::initial_seek(mounted.resume_at)
                } else {
                    None
                });
                let item_owner = Owner::current();
                let loaded = signal(false);
                let progressed = signal(false);
                let initial_load = stored(transport::LoadLatch::default());
                let seek = stored(transport::SeekLatch::default());
                let native_position = signal((0.0_f64, 0.0_f64));
                let buffer_state = stored(Buffering::default());
                let completion = stored(completion::Gate::default());
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
                let recorder = progress::observe(session, mounted.history, routed);
                effect(move || {
                    if routed.get()
                        && !is_incoming.get()
                        && current_source()
                        && let Some(target) = recorder.take_handoff()
                    {
                        log::debug!("[Player] resume from other route at {target:.3}s");
                        if (native_position.get_untracked().0 - target).abs() > 0.3
                            || pending_seek.get_untracked().is_some()
                        {
                            pending_seek.set(Some(target));
                        }
                    }
                });
                effect(move || {
                    if paused.get() || !play_allowed.get() {
                        recorder.flush();
                    }
                });
                effect(move || {
                    if is_incoming.get() {
                        let Some(target) = incoming_seek.get() else {
                            return;
                        };
                        if loaded.get() && seek.with(|seek| seek.dispatch(target)) {
                            control.seek(target);
                        }
                        return;
                    }
                    if pending_seek.get().is_none() {
                        seek.with(transport::SeekLatch::clear);
                        return;
                    }
                    if current_source()
                        && loaded.get()
                        && let Some(target) = pending_seek.get()
                    {
                        let native_duration = native_position.get().1;
                        let Some(bounded) = transport::interactive_seek(target, native_duration) else {
                            pending_seek.set(None);
                            buffering.set(false);
                            return;
                        };
                        if bounded != target {
                            log::debug!("[Player] seek adjusted requested={target:.3}s dispatched={bounded:.3}s duration={native_duration:.3}s");
                            pending_seek.set(Some(bounded));
                            return;
                        }
                        buffering.set(true);
                        if seek.with(|seek| seek.dispatch(target)) {
                            control.seek(target);
                        }
                    }
                });
                let surface = view().absolute().inset_0().child(super::renderer::mount(
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
                        interval: computed(move || {
                            if is_incoming.get() {
                                transport::INCOMING_PROGRESS_INTERVAL_MS
                            } else {
                                250.0
                            }
                        }),
                        volume,
                        muted: computed(move || {
                            muted.get() || pending_seek.get().is_some() || is_incoming.get()
                        }),
                        opacity: computed(move || {
                            transport::incoming_opacity(
                                !is_incoming.get() || incoming_revealing.get(),
                            )
                        }),
                        motion: layer_motion,
                        paused: {
                            let last_native_paused = stored(None::<bool>);
                            computed(move || {
                                if is_incoming.get() {
                                    let ready = incoming_ready.get();
                                    let revealing = incoming_revealing.get();
                                    let effective =
                                        transport::incoming_paused(ready && !revealing);
                                    if last_native_paused.replace(Some(effective)) != Some(effective)
                                    {
                                        log::info!(
                                            "[Player] nativePaused={effective} role=incoming ready={ready} revealing={revealing}"
                                        );
                                    }
                                    effective
                                } else {
                                    let handoff = chrome.silent_handoff();
                                    let allowed = play_allowed.get();
                                    let current = source.with(|source| {
                                        source.is_some_and(|source| {
                                            source.lease.url() == source_url.get()
                                        })
                                    }) && source.error().is_none();
                                    let user_paused = paused.get();
                                    let effective = transport::outgoing_paused(
                                        handoff,
                                        allowed,
                                        current,
                                        user_paused,
                                    );
                                    if last_native_paused.replace(Some(effective)) != Some(effective) {
                                        log::info!(
                                            "[Player] nativePaused={effective} role=outgoing handoff={handoff} playAllowed={allowed} current={current} userPaused={user_paused}"
                                        );
                                    }
                                    effective
                                }
                            })
                        },
                        active: computed(move || {
                            play_allowed.get() && (current_source() || is_incoming.get())
                        }),
                        resize_mode,
                    },
                    super::renderer::Callbacks {
                        load: Box::new(move |info: &VideoInfo| {
                            if !item_owner.is_live()
                                || (!current_source() && !is_incoming.get_untracked())
                            {
                                return;
                            }
                            native_position.update(|position| position.1 = info.duration.into());
                            let duration =
                                effective_duration(mounted.expected_duration, info.duration.into());
                            log::info!(
                                "[Player] loaded series={} episode={} duration={duration:.2}s size={}x{} incoming={}",
                                finished_series.get(),
                                finished_episode,
                                info.size.width.0,
                                info.size.height.0,
                                is_incoming.get_untracked()
                            );
                            if is_incoming.get_untracked() {
                                if let Some(at) = initial_load.with(|load| {
                                    load.seek(incoming_seek.get(), mounted.resume_at, duration)
                                }) {
                                    incoming_seek.replace(transport::initial_seek(at));
                                }
                                loaded.set(true);
                                return;
                            }
                            let size =
                                (f64::from(info.size.width.0), f64::from(info.size.height.0));
                            video_size.set(size);
                            if let Some(modes) = crate::window_modes::WindowModes::current() {
                                modes.refit(crate::window_modes::portrait_aspect(size.0, size.1));
                            }
                            let requested = pending_seek.get_untracked();
                            if let Some(at) = initial_load
                                .with(|load| load.seek(requested, mounted.resume_at, duration))
                            {
                                position.set((0.0, duration));
                                let target =
                                    if at == 0.0 && requested.unwrap_or(mounted.resume_at) > 0.0 {
                                        Some(0.0)
                                    } else {
                                        transport::initial_seek(at)
                                    };
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
                            if !item_owner.is_live()
                                || !progress.current_time.is_finite()
                                || progress.current_time < 0.0
                            {
                                return;
                            }
                            if is_incoming.get_untracked() {
                                if loaded.get_untracked() {
                                    progressed.set(true);
                                    if let Some(target) = incoming_seek.get()
                                        && seek.with(|seek| {
                                            seek.confirmed(target, progress.current_time.into())
                                        })
                                    {
                                        incoming_seek.replace(None);
                                    }
                                    mark_incoming_picture(
                                        true,
                                        true,
                                        incoming_seek.get().is_some(),
                                        true,
                                        incoming_ready,
                                    );
                                }
                                return;
                            }
                            if loaded.get_untracked()
                                && current_source()
                                && !is_incoming.get_untracked()
                            {
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
                                    if f64::from(progress.current_time) > previous.0 {
                                        relay_notice.set(SharedString::default());
                                    }
                                    position.set((progress.current_time.into(), duration));
                                    if routed.get_untracked() {
                                        recorder.record(progress.current_time.into(), duration);
                                    }
                                }
                            }
                        }),
                        buffer: Box::new(move |value: &bool| {
                            if !item_owner.is_live() {
                                return;
                            }
                            if current_source() && !is_incoming.get_untracked() {
                                let seeking = pending_seek.get_untracked().is_some();
                                let next_buffering = buffer_state
                                    .with(|state| state.native_event(*value, seeking));
                                if buffering.get_untracked() != next_buffering {
                                    log::info!(
                                        "[Player] buffering={next_buffering} native={} seeking={seeking}",
                                        *value
                                    );
                                }
                                buffering.set(next_buffering);
                            }
                        }),
                        error: Box::new(move |message: &str| {
                            if !item_owner.is_live() {
                                return;
                            }
                            if current_source() || is_incoming.get_untracked() {
                                let incoming_layer = is_incoming.get_untracked();
                                log::warn!(
                                    "[Player] nativeError incoming={incoming_layer} message={message}"
                                );
                                if incoming_layer {
                                    incoming.set(None);
                                    staged.set(None);
                                    incoming_ready.set(false);
                                    incoming_opaque.set(false);
                                    chrome.end_handoff();
                                }
                                failure.set(message.to_owned().into());
                                pending_seek.set(None);
                                buffering.set(false);
                            }
                        }),
                        end: Box::new(move || {
                            if !item_owner.is_live() || is_incoming.get_untracked() || !routed.get_untracked() {
                                return;
                            }
                            if !current_source() && !chrome.silent_handoff() {
                                return;
                            }
                            let pending = pending_seek.get_untracked();
                            let (_, duration) = native_position.get_untracked();
                            if completion::end_is_blocked(
                                paused.get_untracked(),
                                pending,
                                effective_duration(mounted.expected_duration, duration),
                                chrome.silent_handoff(),
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
                            pending_seek.set(None);
                            let auto_advance = play_allowed.get_untracked()
                                && !paused.get_untracked()
                                && !download_open.get_untracked()
                                && session.with(|session| session.state.settings().auto_next_episode);
                            log::info!("[Player] finished episode={finished_episode} autoAdvance={auto_advance} pendingTailSeek={}", pending.is_some());
                            recorder.complete();
                            lifecycle::delete_finished(
                                session,
                                finished_series.get(),
                                finished_episode,
                                notice,
                            );
                            if auto_advance {
                                next();
                            } else {
                                paused.set(true);
                            }
                        }),
                    },
                ));
                match entrance {
                    Some(entrance) => surface.bind_motion(&entrance),
                    None => surface,
                }
            },
        ))
        .child(overlay())
        .map(move |surface| gestures.bind(surface))
}

fn mark_incoming_picture(
    incoming: bool,
    loaded: bool,
    seeking: bool,
    progressed: bool,
    incoming_ready: Signal<bool>,
) {
    if incoming && transport::incoming_picture_ready(loaded, seeking, progressed) {
        incoming_ready.set(true);
    }
}

fn effective_duration(expected: Option<f64>, native: f64) -> f64 {
    if native.is_finite() && native > 0.0 {
        native
    } else {
        expected.unwrap_or(0.0)
    }
}
