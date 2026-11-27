mod buffering;
mod chrome;
mod completion;
mod download;
pub(crate) mod enhancement;
mod error_state;
mod info;
mod interaction;
pub(crate) mod presentation;
pub(crate) use info::OverlayMeta;
pub(crate) use interaction::SocialMemory;
pub(crate) use interaction::danmaku::DanmakuPreferences;
pub(crate) use interaction::detail_social;
pub(crate) use interaction::reviews_with_header;
mod display_sleep;
mod keyboard;
mod lifecycle;

mod native;
pub(crate) mod navigation;
mod preferences;
mod prepare;
pub(crate) use preferences::PreferenceSignals;
mod progress;
pub(crate) use progress::ProgressMemory;
mod relay;
mod renderer;
mod season_menu;
pub(crate) mod stage;
mod start;
mod start_action;
pub(crate) use start_action::{StartAction, start_action};
mod transport;
mod window_controls;
pub(crate) use start::start_episode;
pub(crate) use start::{resolve_history_start, resolve_start};

use crate::{
    bridge,
    features::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;

const PREFETCH_RECHECK_REMAINING_SECS: f64 = 15.0;

pub(crate) fn page(session: SessionHandle, nav: Navigation, id: String, episode: u32) -> Screen {
    let owner = Route::Player(id.clone(), episode);
    let modes = crate::window_modes::WindowModes::current();
    crate::shell::page(nav, move || tr("nav.player.title"))
        .header_visible(move || is_desktop() || modes.is_none_or(|modes| !modes.state().landscape))
        .child(embedded(
            session,
            nav,
            id,
            episode,
            owner,
            computed(OverlayMeta::default),
            None,
        ))
}

/// Mount the same player in a parent route that owns its visibility.
pub(crate) fn embedded(
    session: SessionHandle,
    nav: Navigation,
    id: String,
    episode: u32,
    owner_route: Route,
    overlay: Memo<OverlayMeta>,
    feed: Option<navigation::FeedNavigation>,
) -> impl IntoElement {
    content(session, nav, id, episode, owner_route, overlay, feed)
}

fn content(
    session: SessionHandle,
    nav: Navigation,
    id: String,
    episode: u32,
    page_route: Route,
    overlay: Memo<OverlayMeta>,
    feed: Option<navigation::FeedNavigation>,
) -> impl IntoElement {
    let selection = signal((id, episode));
    let direction = feed.map_or_else(|| signal(1), navigation::FeedNavigation::direction);
    let definition = signal(None::<u32>);
    let reset_definition = signal(false);
    let paused = signal(false);
    let preferences = session.playback_preferences();
    let rate = preferences.rate;
    let position = signal((0.0_f64, 0.0_f64));
    let video_size = signal((0.0_f64, 0.0_f64));
    let comments_open = signal(false);
    let geometry = crate::features::player::presentation::PlayerGeometry {
        comment_anchor: element_ref(),
        comment_trigger: element_ref(),
        source: video_size,
        viewport: signal((0.0, 0.0)),
        stage: signal((0.0, 0.0)),
        toolbar_height: signal(0.0),
        binge_height: signal(0.0),
        comments_open,
    };
    provide_context(geometry);
    let window_modes = crate::window_modes::WindowModes::current();
    let has_surface = signal(false);
    let buffering = signal(false);
    let pending_seek = signal(None::<f64>);
    let failure = signal(SharedString::default());
    let notice = signal(SharedString::default());
    let relay_notice = signal(SharedString::default());
    let staged_playback = stored(None::<(String, u32, prepare::Playback)>);
    let staged_layer = signal(None::<native::VideoSource>);
    let stage_key = stored(None::<(String, u32)>);
    let stage_task = stored(None::<nsh_core::element::Spawned<()>>);
    let source = resource(
        move || Some((selection.get(), definition.get(), reset_definition.get())),
        move |((id, episode), definition, reset_definition), _| {
            let session = session.with(|session| session.clone());
            let start_position = pending_seek.get_untracked();
            let reused = staged_playback.with(|slot| {
                slot.take_if(|(series, staged_episode, _)| {
                    transport::reuse_staged(Some((series.as_str(), *staged_episode)), &id, episode)
                })
                .map(|(_, _, playback)| playback)
            });
            if reused.is_some() {
                staged_layer.set(None);
            }
            if let Some(playback) = reused {
                return bridge::request(async move { Ok(playback) });
            }
            bridge::request(async move {
                prepare::prepare(
                    session,
                    id,
                    episode,
                    definition,
                    reset_definition,
                    start_position,
                )
                .await
            })
        },
    );
    let routed = computed(move || {
        nav.routes()
            .with(|routes| routes.last() == Some(&page_route))
    });
    effect(move || {
        if source.loading() {
            failure.set(SharedString::default());
            buffering.set(false);
        }
    });
    let retry = move || {
        error_state::retry(source.error().as_ref(), || {
            if let Some((vid, active_def)) = source.with(|source| {
                source.map(|playback| (playback.history.vid.clone(), playback.active_definition))
            }) {
                hongguo_service::play_service::online::invalidate_stream(&vid, active_def);
            }
            pending_seek.set(Some(position.get_untracked().0));
            source.refetch();
        });
    };
    let visible =
        computed(move || lifecycle() == nsh_core::platform::Lifecycle::Active && routed.get());
    crate::shell::PlayerOverlay::bind(
        visible,
        comments_open,
        computed(move || window_modes.is_some_and(|modes| modes.state().landscape)),
    );
    let play_allowed = computed(move || {
        session.identity();
        lifecycle::may_play(
            is_desktop(),
            lifecycle(),
            routed.get(),
            session.with(|session| session.state.settings().pause_on_minimize),
            window_modes.is_none_or(|modes| modes.exposed()),
        ) && !crate::boss_key::hidden()
            && window_modes.is_none_or(|modes| modes.can_play())
    });
    let last_gate = stored(None::<(bool, nsh_core::platform::Lifecycle, bool, bool, bool, bool)>);
    effect(move || {
        let snapshot = (
            play_allowed.get(),
            lifecycle(),
            routed.get(),
            window_modes.is_none_or(|modes| modes.exposed()),
            window_modes.is_none_or(|modes| modes.can_play()),
            crate::boss_key::hidden(),
        );
        if last_gate.replace(Some(snapshot)) != Some(snapshot) {
            let (allowed, life, routed, exposed, can_play, boss_hidden) = snapshot;
            log::info!(
                "[Player] gate playAllowed={allowed} lifecycle={life:?} routed={routed} exposed={exposed} canPlay={can_play} bossHidden={boss_hidden}"
            );
        }
    });
    let last_paused = stored(None::<bool>);
    effect(move || {
        let now = paused.get();
        if last_paused.replace(Some(now)) != Some(now) {
            let (series, episode) = selection.get_untracked();
            log::info!("[Player] paused={now} series={series} episode={episode}");
        }
    });
    display_sleep::bind(move || {
        let covering_display = window_modes.is_some_and(|modes| {
            let state = modes.state();
            state.fullscreen || state.landscape
        });
        play_allowed.get() && !paused.get() && covering_display
    });
    let danmaku = interaction::danmaku::observe(
        session,
        move || {
            if source.loading() || source.error().is_some() {
                None
            } else {
                source.with(|source| source.map(|source| source.social.clone()))
            }
        },
        position,
        interaction::danmaku::Motion::new(
            play_allowed,
            paused,
            computed(move || source.loading()),
            buffering,
            pending_seek,
        ),
        play_allowed,
    );
    let menu_open = signal(None::<stage::Panel>);
    let download_open = signal(false);
    let enhancement = enhancement::Controller::new(preferences.enhancement);
    provide_context(enhancement);
    let panels_open = computed(move || {
        menu_open.get().is_some()
            || download_open.get()
            || comments_open.get()
            || enhancement.open.get()
            || danmaku.settings().open.get()
            || use_context::<crate::navigation::AccountPrompt>().is_some_and(|prompt| prompt.open())
    });
    let chrome_active = computed(move || {
        routed.get()
            && !crate::boss_key::hidden()
            && if is_desktop() {
                lifecycle() != nsh_core::platform::Lifecycle::Background
                    && window_modes.is_none_or(|modes| modes.exposed() && modes.can_play())
            } else {
                lifecycle() == nsh_core::platform::Lifecycle::Active
            }
    });
    let chrome = chrome::Chrome::new(chrome::State {
        active: chrome_active,
        paused,
        panel_open: panels_open,
    })
    .with_handoff(feed.map(navigation::FeedNavigation::quiet_handoff));
    provide_context(chrome);
    let apply_series = move |id: String, episode: u32| {
        if !transport::change_source(&selection.get_untracked(), &(id.clone(), episode)) {
            return;
        }
        chrome.begin_handoff();
        chrome.arm_status_grace();
        adopt_staged_playback(staged_playback, staged_layer, source, &id, episode);
        batch(|| {
            selection.set((id, episode));
            position.set((0.0, 0.0));
            pending_seek.set(None);
            reset_definition.set(false);
            paused.set(false);
            menu_open.set(None);
            comments_open.set(false);
            danmaku.settings().open.set(false);
            enhancement.open.set(false);
        });
    };
    if let Some(feed) = feed {
        effect(move || {
            let Some((id, episode)) = feed.playable().get() else {
                return;
            };
            apply_series(id, episode);
        });
    }
    let relay = relay::observe(
        session,
        selection,
        relay::State {
            active: play_allowed,
            downloading: computed(move || download_open.get()),
            source: computed(move || {
                if source.loading() || source.error().is_some() {
                    return None;
                }
                source.with(|ready| {
                    ready.map(|ready| relay::SourceKey {
                        series_id: selection.get().0,
                        episode: selection.get().1,
                        media: ready.lease.url().to_owned(),
                    })
                })
            }),
            paused,
        },
        feed,
        relay_notice,
        move |series, episode| apply_series(series.series_id, episode),
    );
    let relay_pending = relay.pending();
    let busy = computed(move || source.loading() || relay_pending.get());
    let next = move || {
        if busy.get() {
            return;
        }
        direction.set(1);
        if let Some(next) = source.with(|source| source.and_then(|source| source.next_episode)) {
            chrome.begin_handoff();
            chrome.arm_status_grace();
            let series = selection.get_untracked().0;
            adopt_staged_playback(staged_playback, staged_layer, source, &series, next);
            batch(|| {
                selection.update(|(_, episode)| *episode = next);
                position.set((0.0, 0.0));
                pending_seek.set(None);
                reset_definition.set(false);
                paused.set(false);
            });
        } else if !relay.advance() {
            paused.set(true);
        }
    };
    effect(move || {
        if !source.loading()
            && !paused.get()
            && pending_seek.get().is_none()
            && position.get().0 > 0.0
        {
            chrome.mark_played();
        }
    });
    let prefetch_armed = stored(None::<(String, u32)>);
    effect(move || {
        let (series, episode) = selection.get();
        let (at, duration) = position.get();
        if source.loading()
            || paused.get()
            || duration <= 0.0
            || duration - at > PREFETCH_RECHECK_REMAINING_SECS
        {
            return;
        }
        let Some(next_episode) =
            source.with(|source| source.and_then(|source| source.next_episode))
        else {
            return;
        };
        let key = (series.clone(), next_episode);
        if prefetch_armed.with(|armed| armed.as_ref() == Some(&key)) {
            return;
        }
        prefetch_armed.replace(Some(key));
        let state = session.with(|session| session.state.clone());
        hongguo_service::play_service::schedule_following_episodes(&state, &series, episode);
    });
    let player_owner = Owner::current();
    effect(move || {
        if chrome.silent_handoff()
            || source.loading()
            || paused.get()
            || pending_seek.get().is_some()
            || comments_open.get()
            || download_open.get()
            || !play_allowed.get()
            || !session.with(|session| session.state.settings().auto_next_episode)
        {
            return;
        }
        let (series, episode) = selection.get();
        let (at, duration) = position.get();
        let remaining = duration - at;
        let Some(next_episode) =
            source.with(|source| source.and_then(|source| source.next_episode))
        else {
            return;
        };
        let key = (series.clone(), next_episode);
        let already = stage_key.with(|armed| armed.as_ref() == Some(&key));
        if !transport::should_stage_next(remaining, true, already) {
            return;
        }
        stage_key.replace(Some(key));
        if let Some(task) = stage_task.replace(None) {
            task.cancel();
        }
        let origin = (series.clone(), episode);
        let definition = definition.get();
        let session = session.with(|session| session.clone());
        log::info!(
            "[Player] stageNext series={} current={} next={} remaining={remaining:.2}s",
            origin.0,
            origin.1,
            next_episode
        );
        let task = player_owner.run(move || {
            spawn(async move {
                let prepared = bridge::request(prepare::prepare(
                    session,
                    origin.0.clone(),
                    next_episode,
                    definition,
                    false,
                    None,
                ))
                .await;
                if selection.get_untracked() != origin {
                    log::info!(
                        "[Player] stageNext discarded series={} next={} reason=selectionChanged",
                        origin.0,
                        next_episode
                    );
                    return;
                }
                match prepared {
                    Ok(playback) => {
                        log::info!(
                            "[Player] stageNext ready series={} next={}",
                            origin.0,
                            next_episode
                        );
                        staged_playback.replace(Some((origin.0, next_episode, playback.clone())));
                        let layer = native::VideoSource::from_playback(&playback);
                        let settings = enhancement.settings.get_untracked();
                        staged_layer.set(
                            layer
                                .can_overlap_picture(settings.anime4k_on || settings.sharpen_on)
                                .then_some(layer),
                        );
                    }
                    Err(error) => {
                        log::warn!(
                            "[Player] stageNext failed series={} next={}: {error}",
                            origin.0,
                            next_episode
                        );
                    }
                }
            })
        });
        stage_task.replace(Some(task));
    });
    effect(move || {
        if chrome.silent_handoff()
            || source.loading()
            || paused.get()
            || pending_seek.get().is_some()
            || comments_open.get()
            || download_open.get()
            || !play_allowed.get()
            || !session.with(|session| session.state.settings().auto_next_episode)
        {
            return;
        }
        let (at, duration) = position.get();
        if !transport::should_hold_outgoing(duration - at, duration, true) {
            return;
        }
        if staged_layer.with(|layer| layer.is_none()) {
            return;
        }
        if source
            .with(|source| source.and_then(|source| source.next_episode))
            .is_none()
        {
            return;
        }
        next();
    });
    let seasons = season_menu::observe(
        session,
        selection,
        menu_open,
        relay.related,
        relay.source_key(),
    );
    let muted = preferences.muted;
    let volume = preferences.volume;
    let menus = stage::MenuState {
        open: menu_open,
        definitions: computed(move || {
            source.with(|source| {
                source
                    .map(|source| source.definitions.clone())
                    .unwrap_or_default()
            })
        }),
        selected_definition: computed(move || {
            source.with(|source| {
                source.and_then(|source| {
                    (source.online && !source.automatic_definition && source.active_definition > 0)
                        .then_some(source.active_definition)
                })
            })
        }),
        seasons: seasons.state,
        episodes: computed(move || {
            source.with(|source| {
                source
                    .map(|source| source.episode_indices.clone())
                    .unwrap_or_default()
            })
        }),
        current_episode: computed(move || selection.get().1),
        volume,
        muted,
    };
    let select_episode = move |episode| {
        if busy.get() || !transport::change_episode(selection.get_untracked().1, episode) {
            return;
        }
        chrome.begin_handoff();
        chrome.arm_status_grace();
        let series = selection.get_untracked().0;
        adopt_staged_playback(staged_playback, staged_layer, source, &series, episode);
        batch(|| {
            direction.set(if episode < selection.get_untracked().1 {
                -1
            } else {
                1
            });
            selection.update(|(_, selected)| *selected = episode);
            position.set((0.0, 0.0));
            pending_seek.set(None);
            reset_definition.set(false);
            paused.set(false);
        })
    };
    let gestures = navigation::Controller::new(
        navigation::State {
            active: computed(move || visible.get() && !busy.get()),
            blocked: panels_open,
            series: computed(move || selection.get().0),
            direction,
        },
        feed,
        move |direction| {
            if direction > 0 {
                next();
            } else if let Some(previous) =
                source.with(|source| source.and_then(|source| source.previous_episode))
            {
                select_episode(previous);
            }
        },
    )
    .on_activity(move || chrome.wake());
    let in_binge = gestures.in_binge();
    nav.on_back(
        computed(move || routed.get() && in_binge.get()),
        move || {
            if let Some(feed) = feed {
                feed.clear_lock();
            }
        },
    );
    effect(move || {
        if !is_desktop()
            && visible.get()
            && !panels_open.get()
            && (in_binge.get() || window_modes.is_some_and(|modes| modes.state().landscape))
        {
            on_back_press(move || {
                chrome.wake();
                if let Some(modes) = window_modes.filter(|modes| modes.state().landscape) {
                    modes.set_landscape(false);
                } else {
                    gestures.exit_binge();
                }
                true
            });
        }
    });
    effect(move || {
        if !visible.get()
            || panels_open.get()
            || crate::shortcuts::Shortcuts::current().is_some_and(|keys| keys.is_capturing())
            || (!in_binge.get() && !window_modes.is_some_and(|modes| modes.immersive()))
        {
            return;
        }
        on_key_with(
            "escape",
            KeyOptions {
                when: nsh_core::keymap::When::NotEditing,
                repeat: false,
                ..Default::default()
            },
            move |_| {
                chrome.wake();
                if let Some(modes) = window_modes.filter(|modes| modes.immersive()) {
                    modes.exit();
                } else {
                    gestures.exit_binge();
                }
            },
        );
    });
    keyboard::bind(
        &crate::shortcuts::Action::ALL,
        computed(move || {
            visible.get()
                && !busy.get()
                && menu_open.get().is_none()
                && !danmaku.settings().open.get()
                && !download_open.get()
                && !use_context::<crate::navigation::AccountPrompt>()
                    .is_some_and(|prompt| prompt.open())
        }),
        move |action| {
            chrome.wake();
            match action {
                crate::shortcuts::Action::PlayPause => paused.update(|paused| *paused = !*paused),
                crate::shortcuts::Action::SeekBack | crate::shortcuts::Action::SeekForward => {
                    if pending_seek.get_untracked().is_some() {
                        return;
                    }
                    let (at, duration) = position.get_untracked();
                    let forward = action == crate::shortcuts::Action::SeekForward;
                    if let Some(target) = transport::skip_target(at, duration, forward) {
                        pending_seek.set(Some(target));
                    }
                }
                crate::shortcuts::Action::StepNext => gestures.step_keyboard(1),
                crate::shortcuts::Action::StepPrev => gestures.step_keyboard(-1),
            }
        },
    );
    let video = native::mount(
        native::State {
            session,
            routed,
            source,
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
            staged: staged_layer,
        },
        move || danmaku.overlay(),
        next,
    );
    let information = info::observe(source, overlay);
    let controls_state = stage::ControlState {
        geometry,
        caption: computed(move || information.get().into()),
        paused,
        rate,
        position,
        quality: computed(move || {
            source.with(|source| {
                source.map_or_else(
                    || tr("player.definitionAuto"),
                    |source| {
                        if !source.online {
                            source.lease.video_short_edge.map_or_else(
                                || tr("player.definitionLocal"),
                                |height| format!("{height}P").into(),
                            )
                        } else if source.automatic_definition || source.active_definition == 0 {
                            tr("player.definitionAuto")
                        } else {
                            format!("{}P", source.active_definition).into()
                        }
                    },
                )
            })
        }),
        busy,
        seek_pending: computed(move || pending_seek.get().is_some()),
        has_previous: computed(move || {
            source.with(|source| source.is_some_and(|source| source.previous_episode.is_some()))
        }),
        has_next: computed(move || {
            source.with(|source| source.is_some_and(|source| source.next_episode.is_some()))
        }),
        episode_count: computed(move || {
            source.with(|source| source.map_or(0, |source| source.history.episode_cnt))
        }),
        can_binge: gestures.can_binge(),
        in_binge,
        advanced: stage::AdvancedState {
            enhancement: computed(renderer::pixel_supported),
            window_modes: computed(move || {
                is_desktop() && window_modes.is_some_and(|modes| !modes.pending())
            }),
            incognito: computed(move || window_modes.is_some_and(|modes| modes.state().incognito)),
        },
        danmaku_settings: danmaku.settings(),
        danmaku_enabled: danmaku.enabled(),
        menus,
    };
    let action = move |action| {
        chrome.wake();
        use stage::ControlAction;
        match action {
            ControlAction::Next => next(),
            ControlAction::Previous => {
                if let Some(previous) =
                    source.with(|source| source.and_then(|source| source.previous_episode))
                {
                    select_episode(previous);
                }
            }
            ControlAction::Seek(at) => {
                if !busy.get() && at.is_finite() && at >= 0.0 {
                    let duration = position.get_untracked().1;
                    if let Some(target) = transport::interactive_seek(at, duration) {
                        pending_seek.set(Some(target));
                    }
                }
            }
            ControlAction::SetRate(value) => {
                if value.is_finite()
                    && (preferences::MIN_RATE..=preferences::MAX_RATE).contains(&value)
                {
                    rate.set(value);
                }
            }
            ControlAction::SetDefinition(requested) => {
                if !busy.get()
                    && let Some(change) = transport::change_definition(
                        menus.selected_definition.get_untracked(),
                        requested,
                        position.get_untracked().0,
                    )
                {
                    batch(|| {
                        pending_seek.set(Some(change.resume_at));
                        definition.set(change.definition);
                        reset_definition.set(change.reset_definition);
                    });
                }
            }
            ControlAction::SelectEpisode(episode) => select_episode(episode),
            ControlAction::EnterBinge => gestures.enter_binge(),
            ControlAction::ExitBinge => gestures.exit_binge(),
            ControlAction::RetrySeasons => seasons.retry(),
            ControlAction::ReserveSeason(id) => seasons.reserve(session, nav, id),
            ControlAction::SwitchSeason(id) => {
                if busy.get() || id == selection.get_untracked().0 {
                    return;
                }
                seasons.switch(id, move |series, episode| {
                    if !visible.get_untracked() {
                        return;
                    }
                    if let Some(feed) = feed {
                        feed.lock_series(series.series_id.clone());
                    }
                    apply_series(series.series_id.clone(), episode);
                    if let Some(feed) = feed {
                        let _ = feed.request_goto(series, episode);
                    }
                });
            }
            ControlAction::SetVolume(value) => {
                if value.is_finite() {
                    batch(|| {
                        let value = value.clamp(0.0, 1.0);
                        volume.set(value);
                        muted.set(value == 0.0);
                    });
                }
            }
            ControlAction::Download => download_open.set(true),
            ControlAction::ToggleMute => muted.update(|muted| *muted = !*muted),
            ControlAction::Fullscreen => {
                menu_open.set(None);
                comments_open.set(false);
                danmaku.settings().open.set(false);
                enhancement.open.set(false);
                chrome.hovered(false);
                if let Some(modes) = window_modes {
                    modes.toggle_fullscreen();
                } else {
                    notice.set(tr("nsh.windowModeUnavailable"));
                }
            }
            ControlAction::Danmaku => danmaku.toggle(),
            ControlAction::Advanced(control) => {
                if matches!(
                    control,
                    stage::AdvancedControl::MiniWindow
                        | stage::AdvancedControl::Portrait
                        | stage::AdvancedControl::WebFullscreen
                ) {
                    menu_open.set(None);
                    comments_open.set(false);
                    danmaku.settings().open.set(false);
                    enhancement.open.set(false);
                    chrome.hovered(false);
                    chrome.leave();
                }
                let window_action = |action: &dyn Fn(crate::window_modes::WindowModes)| {
                    if let Some(modes) = window_modes {
                        action(modes);
                    } else {
                        notice.set(tr("nsh.windowModeUnavailable"));
                    }
                };
                match control {
                    stage::AdvancedControl::DanmakuSettings => danmaku.toggle_settings(),
                    stage::AdvancedControl::Enhancement => {
                        menu_open.set(None);
                        enhancement.open.update(|open| *open = !*open);
                    }
                    stage::AdvancedControl::MiniWindow => {
                        window_action(&|modes| modes.toggle_mini())
                    }
                    stage::AdvancedControl::Incognito => {
                        window_action(&|modes| modes.toggle_incognito())
                    }
                    stage::AdvancedControl::Portrait => window_action(&|modes| {
                        let (width, height) = video_size.get_untracked();
                        modes.toggle_portrait(crate::window_modes::portrait_aspect(width, height));
                    }),
                    stage::AdvancedControl::WebFullscreen => {
                        window_action(&|modes| modes.toggle_frameless())
                    }
                }
            }
        }
    };
    stage::surface(
        video,
        stage::info(information, move || {
            if let Some(id) =
                source.with(|source| source.map(|source| source.history.series_id.clone()))
            {
                nav.push(Route::Detail(id));
            }
        }),
        interaction::panel(
            session,
            nav,
            move || source.with(|source| source.map(|source| source.social.clone())),
            comments_open,
        ),
        view()
            .absolute()
            .inset_0()
            .pointer_events(PointerEvents::BoxNone)
            .child(
                stage::controls(controls_state, action, move |previous| {
                    danmaku.controls(session, previous)
                })
                .style_when(
                    move || {
                        window_modes.is_some_and(|modes| {
                            modes.state().mode != crate::window_modes::Mode::Normal
                        })
                    },
                    |style| style.hidden(),
                ),
            )
            .child(window_controls::controls(
                controls_state,
                information,
                video_size,
                action,
            )),
        view()
            .gap_3()
            .child(when(
                move || {
                    !chrome.silent_handoff()
                        && transport::cold_status_loading(
                            has_surface.get(),
                            source.with(|source| source.is_some()),
                        )
                },
                move || ui::status(move || tr("common.loading")),
            ))
            .child(when(
                move || source.error().is_some() || !failure.get().is_empty(),
                move || {
                    view().child(error_state::panel(
                        computed(move || source.error()),
                        failure,
                        retry,
                    ))
                },
            ))
            .child(when(
                move || (buffering.get() || chrome.silent_handoff()) && !chrome.suppress_status(),
                move || ui::status(move || tr("player.bufferingShort")),
            ))
            .child(ui::status(notice))
            .child(ui::status(relay_notice))
            .child(ui::status(move || {
                window_modes.map_or_else(SharedString::default, |modes| modes.error())
            }))
            .child(ui::status(move || danmaku.notice())),
        computed(move || controls_state.binge_bar_visible()),
    )
    .child(download::sheet(session, nav, download_open, source))
}

fn adopt_staged_playback(
    staged_playback: Stored<Option<(String, u32, prepare::Playback)>>,
    staged_layer: Signal<Option<native::VideoSource>>,
    source: Resource<prepare::Playback, bridge::Failure>,
    series: &str,
    episode: u32,
) {
    let reused = staged_playback.with(|slot| {
        slot.as_ref().and_then(|(sid, ep, playback)| {
            transport::reuse_staged(Some((sid.as_str(), *ep)), series, episode)
                .then(|| playback.clone())
        })
    });
    if let Some(playback) = reused {
        source.mutate(playback);
        return;
    }
    staged_playback.replace(None);
    staged_layer.set(None);
}
