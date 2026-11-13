mod buffering;
mod chrome;
mod completion;
mod download;
pub(crate) mod enhancement;
mod error_state;
mod info;
mod interaction;
pub(crate) use info::OverlayMeta;
pub(crate) use interaction::SocialMemory;
pub(crate) use interaction::danmaku::DanmakuPreferences;
pub(crate) use interaction::detail_social;
pub(crate) use interaction::{comments_preview, reviews_preview_with_header, reviews_with_header};
mod keyboard;
mod lifecycle;
mod native;
pub(crate) mod navigation;
mod preferences;
mod prepare;
pub(crate) use preferences::PreferenceSignals;
mod progress;
mod relay;
mod renderer;
mod season_menu;
pub(crate) mod stage;
mod start;
mod start_action;
pub(crate) use start_action::{StartAction, start_action};
mod transport;
mod window_controls;
pub(crate) use stage::fixture;
pub(crate) use start::resolve_start;
pub(crate) use start::start_episode;

use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    pages::SessionHandle,
    ui,
};
use nsh_core::prelude::*;

pub(crate) fn page(session: SessionHandle, nav: Navigation, id: String, episode: u32) -> Screen {
    let owner = Route::Player(id.clone(), episode);
    ui::page(nav, move || tr("nav.player.title")).child(embedded(
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
    let definition = signal(None::<u32>);
    let reset_definition = signal(false);
    let paused = signal(false);
    let preferences = session.playback_preferences();
    let rate = preferences.rate;
    let position = signal((0.0_f64, 0.0_f64));
    let video_size = signal((0.0_f64, 0.0_f64));
    let geometry = ui::PlayerGeometry {
        comment_anchor: element_ref(),
        comment_trigger: element_ref(),
        source: video_size,
        viewport: signal((0.0, 0.0)),
    };
    provide_context(geometry);
    let window_modes = crate::window_modes::WindowModes::current();
    let buffering = signal(false);
    let pending_seek = signal(None::<f64>);
    let failure = signal(SharedString::default());
    let notice = signal(SharedString::default());
    let source = resource(
        move || Some((selection.get(), definition.get(), reset_definition.get())),
        move |((id, episode), definition, reset_definition), _| {
            let session = session.with(|session| session.clone());
            bridge::request(async move {
                prepare::prepare(session, id, episode, definition, reset_definition).await
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
            pending_seek.set(Some(position.get_untracked().0));
            source.refetch();
        });
    };
    let visible =
        computed(move || lifecycle() == nsh_core::platform::Lifecycle::Active && routed.get());
    let play_allowed = computed(move || {
        session.identity();
        lifecycle::may_play(
            is_desktop(),
            lifecycle(),
            routed.get(),
            session.with(|session| session.state.settings().pause_on_minimize),
        ) && !crate::boss_key::hidden()
            && window_modes.is_none_or(|modes| modes.can_play())
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
    let comments_open = signal(false);
    let enhancement = enhancement::Controller::new(preferences.enhancement);
    provide_context(enhancement);
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
        notice,
        move |series, episode| {
            batch(|| {
                selection.set((series.series_id, episode));
                position.set((0.0, 0.0));
                pending_seek.set(None);
                reset_definition.set(false);
                paused.set(false);
                menu_open.set(None);
                comments_open.set(false);
                danmaku.settings().open.set(false);
                enhancement.open.set(false);
            });
        },
    );
    let relay_pending = relay.pending();
    let busy = computed(move || source.loading() || relay_pending.get());
    let next = move || {
        if busy.get() {
            return;
        }
        if let Some(next) = source.with(|source| source.and_then(|source| source.next_episode)) {
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
    let panels_open = computed(move || {
        menu_open.get().is_some()
            || download_open.get()
            || comments_open.get()
            || enhancement.open.get()
            || danmaku.settings().open.get()
            || use_context::<crate::navigation::AccountPrompt>().is_some_and(|prompt| prompt.open())
    });
    let chrome = chrome::Chrome::new(chrome::State {
        active: visible,
        paused,
        panel_open: panels_open,
    })
    .with_handoff(feed.map(navigation::FeedNavigation::quiet_handoff));
    provide_context(chrome);
    effect(move || {
        if !source.loading()
            && !paused.get()
            && pending_seek.get().is_none()
            && position.get().0 > 0.0
        {
            chrome.mark_played();
        }
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
                    (!source.automatic_definition).then_some(source.active_definition)
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
        batch(|| {
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
                    const SEEK_SECONDS: f64 = 5.0;
                    let (position, duration) = position.get_untracked();
                    let at = (position
                        + if action == crate::shortcuts::Action::SeekBack {
                            -SEEK_SECONDS
                        } else {
                            SEEK_SECONDS
                        })
                    .max(0.0);
                    pending_seek.set(Some(if duration.is_finite() && duration > 0.0 {
                        at.min(duration)
                    } else {
                        at
                    }));
                }
                crate::shortcuts::Action::StepNext => gestures.step_keyboard(1),
                crate::shortcuts::Action::StepPrev => gestures.step_keyboard(-1),
            }
        },
    );
    let video = native::mount(
        native::State {
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
        },
        move || danmaku.overlay(),
        next,
    );
    let information = info::observe(session, selection, source, overlay);
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
                        if source.automatic_definition {
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
                    let target = if duration.is_finite() && duration > 0.0 {
                        at.min(duration)
                    } else {
                        at
                    };
                    pending_seek.set(Some(target));
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
                        if feed.request_goto(series.clone(), episode) {
                            return;
                        }
                    }
                    batch(|| {
                        selection.set((series.series_id, episode));
                        position.set((0.0, 0.0));
                        pending_seek.set(None);
                        reset_definition.set(false);
                        paused.set(false);
                        menu_open.set(None);
                    });
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
            nav.push(Route::Detail(selection.get_untracked().0))
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
                move || source.loading(),
                move || ui::status(move || tr("common.loading")),
            ))
            .child(when(
                move || source.error().is_some() || !failure.get().is_empty(),
                move || {
                    view().test_id("player-error").child(error_state::panel(
                        computed(move || source.error()),
                        failure,
                        retry,
                    ))
                },
            ))
            .child(when(
                move || buffering.get(),
                move || ui::status(move || tr("player.bufferingShort")),
            ))
            .child(ui::status(notice))
            .child(ui::status(move || {
                window_modes.map_or_else(SharedString::default, |modes| modes.error())
            }))
            .child(ui::status(move || danmaku.notice())),
    )
    .child(download::sheet(session, nav, download_open, source))
}
