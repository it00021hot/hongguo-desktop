use super::{
    compact,
    menus::{self, MenuAction, MenuState},
};
use crate::{
    i18n::{format_duration as duration, tr},
    ui::{self, Icon},
};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct AdvancedState {
    pub enhancement: Memo<bool>,
    pub window_modes: Memo<bool>,
    pub incognito: Memo<bool>,
}

impl AdvancedState {
    #[cfg(test)]
    pub fn unavailable() -> Self {
        let unavailable = computed(|| false);
        Self {
            enhancement: unavailable,
            window_modes: unavailable,
            incognito: unavailable,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ControlState {
    pub(crate) geometry: ui::PlayerGeometry,
    pub(crate) caption: Memo<ui::WindowCaption>,
    pub(crate) paused: Signal<bool>,
    pub(crate) rate: Signal<f32>,
    pub(crate) position: Signal<(f64, f64)>,
    pub(crate) quality: Memo<SharedString>,
    pub(crate) busy: Memo<bool>,
    pub(crate) seek_pending: Memo<bool>,
    pub(crate) has_previous: Memo<bool>,
    pub(crate) has_next: Memo<bool>,
    pub(crate) episode_count: Memo<u32>,
    pub(crate) can_binge: bool,
    pub(crate) in_binge: Memo<bool>,
    pub(crate) advanced: AdvancedState,
    pub(crate) danmaku_settings: ui::DanmakuSettingsState,
    pub(crate) danmaku_enabled: Signal<bool>,
    pub(crate) menus: MenuState,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum AdvancedControl {
    Enhancement,
    MiniWindow,
    DanmakuSettings,
    Incognito,
    Portrait,
    WebFullscreen,
}
#[derive(Clone)]
pub(crate) enum ControlAction {
    Previous,
    Next,
    SetRate(f32),
    SetDefinition(Option<u32>),
    SelectEpisode(u32),
    SetVolume(f32),
    ToggleMute,
    Download,
    Fullscreen,
    Danmaku,
    Advanced(AdvancedControl),
    Seek(f64),
    SwitchSeason(String),
    ReserveSeason(String),
    RetrySeasons,
    EnterBinge,
}

pub(crate) fn controls<E: IntoElement>(
    state: ControlState,
    action: impl Fn(ControlAction) + 'static,
    composer: impl Fn(Memo<ElementRef>) -> E + 'static,
) -> View {
    let composer = stored(Box::new(move |previous| composer(previous).into_element())
        as Box<dyn Fn(Memo<ElementRef>) -> AnyElement>);
    let play_stop = element_ref();
    let more_stop = element_ref();
    let previous_stop = element_ref();
    let next_stop = element_ref();
    let before_composer = computed(move || {
        if state.has_next.get() {
            next_stop
        } else if state.has_previous.get() {
            previous_stop
        } else {
            play_stop
        }
    });
    let action = stored(Box::new(action) as Box<dyn Fn(ControlAction)>);
    let dragging = signal(false);
    let seek = signal(state.position.get_untracked().0);
    let toolbar_wrapped =
        computed(move || super::desktop_toolbar_wrapped(state.geometry.viewport.get().0));
    effect(move || {
        let position = state.position.get();
        if state.busy.get() {
            dragging.set(false);
        }
        if !dragging.get() && !state.seek_pending.get() {
            seek.set(position.0);
        }
    });
    let extra = move |control| action.with(|action| action(ControlAction::Advanced(control)));
    effect(move || {
        if !compact() && state.danmaku_settings.open.get() {
            state.menus.open.set(None);
        }
    });
    effect(move || {
        if !compact() && state.menus.open.get().is_some() {
            state.danmaku_settings.open.set(false);
        }
    });
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::BoxNone)
        .child(ui::player_watch_fullscreen(
            state.geometry,
            computed(|| {
                crate::window_modes::WindowModes::current()
                    .is_some_and(|modes| modes.state().landscape)
            }),
            move || action.with(|action| action(ControlAction::Fullscreen)),
        ))
        .child(when(super::landscape::active, move || {
            ui::player_landscape_header(state.caption, move || {
                action.with(|action| action(ControlAction::Fullscreen))
            })
        }))
        .child(when(compact, move || {
            view()
                .absolute()
                .top_3()
                .right_3()
                .child(ui::mobile_player_more(
                    ui::MobilePlayerMoreState {
                        open: computed(move || state.menus.open.get() == Some(super::Panel::Rate)),
                        rate: state.rate,
                        rates: &super::menu_model::RATES,
                        definitions: state.menus.definitions,
                        selected_definition: state.menus.selected_definition,
                        danmaku_on: state.danmaku_enabled,
                        danmaku: state.danmaku_settings,
                        busy: state.busy,
                        episode_count: state.episode_count,
                        enhancement_available: state.advanced.enhancement,
                    },
                    ui::MobilePlayerMoreEvents {
                        change: Box::new(move |open| {
                            state.menus.set_open(super::Panel::Rate, open);
                        }),
                        action: Box::new(move |intent| {
                            use ui::MobilePlayerMoreAction as Intent;
                            match intent {
                                Intent::SetRate(value) => {
                                    action.with(|action| action(ControlAction::SetRate(value)))
                                }
                                Intent::SetDefinition(value) => action
                                    .with(|action| action(ControlAction::SetDefinition(value))),
                                Intent::ToggleDanmaku => {
                                    action.with(|action| action(ControlAction::Danmaku))
                                }
                                Intent::Episodes => {
                                    state.menus.open.set(Some(super::Panel::Episodes))
                                }
                                Intent::Download => {
                                    action.with(|action| action(ControlAction::Download))
                                }
                                Intent::Enhancement => extra(AdvancedControl::Enhancement),
                            }
                        }),
                    },
                    ui::player_phone_rate(move || format!("{}x", state.rate.get()))
                        .accessibility_label(move || tr("player.playbackRate")),
                    more_stop,
                ))
        }))
        .child(
            view()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .z_index(30)
                .style_when(super::landscape::active, |style| style.mb_keyboard())
                .px(move || px(if compact() { 12.0 } else { 16.0 }))
                .pt(move || px(if compact() { 16.0 } else { 0.0 }))
                .pb(move || px(if compact() { 6.0 } else { 12.0 }))
                .gap(move || px(if compact() { 2.0 } else { 8.0 }))
                .pointer_events(PointerEvents::BoxNone)
                .child(when(
                    move || compact() || super::landscape::active(),
                    move || {
                        view()
                            .flex_row()
                            .justify_between()
                            .child(ui::player_phone_time(move || {
                                duration(state.position.get().0)
                            }))
                            .child(ui::player_phone_time(move || {
                                duration(state.position.get().1)
                            }))
                    },
                ))
                .child(ui::player_seek_rail(
                    ui::player_seek(seek, move || state.position.get().1)
                        .disabled(move || state.busy.get() || state.position.get().1 <= 0.0)
                        .on_change(move |value: &nsh_base::SliderValue| {
                            dragging.set(true);
                            seek.set(value.start());
                        })
                        .on_change_end(move |value: &nsh_base::SliderValue| {
                            let duration = state.position.get_untracked().1;
                            if !state.busy.get_untracked()
                                && duration.is_finite()
                                && duration > 0.0
                                && value.start().is_finite()
                            {
                                action.with(|action| {
                                    action(ControlAction::Seek(value.start().clamp(0.0, duration)))
                                });
                            }
                            dragging.set(false);
                        }),
                    computed(super::landscape::active),
                ))
                .child(when(
                    move || !compact() && !super::landscape::active(),
                    move || {
                        view()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .style_when(toolbar_wrapped, |style| style.flex_wrap())
                            .child(
                                ui::player_toggle_icon(
                                    Icon::Play,
                                    Icon::Pause,
                                    move || !state.paused.get(),
                                    move || {
                                        tr(if state.paused.get() {
                                            "player.play"
                                        } else {
                                            "player.pause"
                                        })
                                    },
                                )
                                .element_ref(&play_stop)
                                .disabled(move || state.busy.get())
                                .on_press(move || state.paused.update(|paused| *paused = !*paused)),
                            )
                            .child(
                                ui::player_icon_button(Icon::Previous, move || {
                                    tr("player.prevEpisode")
                                })
                                .element_ref(&previous_stop)
                                .disabled(move || state.busy.get() || !state.has_previous.get())
                                .on_press(move || {
                                    action.with(|action| action(ControlAction::Previous))
                                }),
                            )
                            .child(
                                ui::player_icon_button(Icon::Next, move || {
                                    tr("player.nextEpisode")
                                })
                                .element_ref(&next_stop)
                                .disabled(move || state.busy.get() || !state.has_next.get())
                                .on_press(move || {
                                    action.with(|action| action(ControlAction::Next))
                                }),
                            )
                            .child(
                                text(move || {
                                    format!(
                                        "{} / {}",
                                        duration(state.position.get().0),
                                        duration(state.position.get().1)
                                    )
                                })
                                .font_family(move || {
                                    nsh_base::theme()
                                        .font_mono
                                        .clone()
                                        .unwrap_or_else(|| "monospace".into())
                                })
                                .text_xs()
                                .line_height(px(16.0))
                                .font_weight(FontWeight::NORMAL)
                                .ml_1(),
                            )
                            .child(composer.with(|render| render(before_composer)))
                            .child(
                                view()
                                    .ml(auto())
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .style_when(toolbar_wrapped, |style| {
                                        style.basis(percentage(1.0)).w_full().justify_end()
                                    })
                                    .child(menus::rate(
                                        state.menus,
                                        state.rate,
                                        state.busy,
                                        labelled(Icon::Speed, move || {
                                            format!("{}x", state.rate.get())
                                        })
                                        .accessibility_label(move || tr("player.playbackRate")),
                                        move |intent| action.with(|action| action(intent.into())),
                                    ))
                                    .child(menus::quality(
                                        state.menus,
                                        state.busy,
                                        labelled(Icon::Quality, move || {
                                            if state.menus.selected_definition.get().is_none() {
                                                tr("player.definitionAuto")
                                            } else {
                                                state.quality.get()
                                            }
                                        })
                                        .accessibility_label(move || tr("player.definition")),
                                        move |intent| action.with(|action| action(intent.into())),
                                    ))
                                    .child(enhancement_control(state.advanced.enhancement))
                                    .child(menus::episodes(
                                        state.menus,
                                        state.busy,
                                        labelled(Icon::Episodes, move || {
                                            format!(
                                                "{} · {}",
                                                tr("player.episodes"),
                                                tr("player.totalEpisodes").replace(
                                                    "{count}",
                                                    &state.episode_count.get().to_string()
                                                )
                                            )
                                        }),
                                        move |intent| action.with(|action| action(intent.into())),
                                    ))
                                    .child(
                                        labelled(Icon::Download, move || tr("player.download"))
                                            .on_press(move || {
                                                action
                                                    .with(|action| action(ControlAction::Download))
                                            }),
                                    )
                                    .child(
                                        ui::player_icon_button(Icon::MiniWindow, move || {
                                            tr("player.miniWindow")
                                        })
                                        .disabled(move || !state.advanced.window_modes.get())
                                        .on_press(move || extra(AdvancedControl::MiniWindow)),
                                    )
                                    .child(ui::player_danmaku_panel(
                                        move || state.danmaku_settings.open.get(),
                                        move |open| {
                                            if open != state.danmaku_settings.open.get_untracked() {
                                                extra(AdvancedControl::DanmakuSettings);
                                            }
                                        },
                                        ui::player_danmaku_settings(state.danmaku_settings.open),
                                        move || ui::danmaku_settings(state.danmaku_settings),
                                    ))
                                    .child(
                                        ui::player_danmaku_toggle(state.danmaku_enabled).on_press(
                                            move || {
                                                action.with(|action| action(ControlAction::Danmaku))
                                            },
                                        ),
                                    )
                                    .child(menus::volume(
                                        state.menus,
                                        ui::player_toggle_icon(
                                            Icon::Volume,
                                            Icon::Muted,
                                            move || {
                                                state.menus.muted.get()
                                                    || state.menus.volume.get() <= 0.0
                                            },
                                            move || tr("player.volume"),
                                        ),
                                        move |intent| action.with(|action| action(intent.into())),
                                    ))
                                    .child(
                                        ui::player_privacy_button(state.advanced.incognito)
                                            .disabled(move || !state.advanced.window_modes.get())
                                            .on_press(move || extra(AdvancedControl::Incognito)),
                                    )
                                    .child(
                                        ui::player_icon_button(Icon::Portrait, move || {
                                            tr("player.portrait")
                                        })
                                        .disabled(move || !state.advanced.window_modes.get())
                                        .on_press(move || extra(AdvancedControl::Portrait)),
                                    )
                                    .child(
                                        ui::player_icon_button(Icon::WebFullscreen, move || {
                                            tr("player.frameless")
                                        })
                                        .disabled(move || !state.advanced.window_modes.get())
                                        .on_press(move || extra(AdvancedControl::WebFullscreen)),
                                    )
                                    .child(
                                        ui::player_icon_button(Icon::Fullscreen, move || {
                                            tr("player.fullscreen")
                                        })
                                        .on_press(
                                            move || {
                                                action.with(|action| {
                                                    action(ControlAction::Fullscreen)
                                                })
                                            },
                                        ),
                                    ),
                            )
                    },
                ))
                .child(when(super::landscape::active, move || {
                    super::landscape::row(
                        state,
                        move |intent| action.with(|action| action(intent)),
                        move |previous| composer.with(|render| render(previous)),
                    )
                }))
                .map(|toolbar| {
                    let toolbar =
                        ui::player_landscape_footer(toolbar, computed(super::landscape::active));
                    if let Some(chrome) = crate::player::chrome::Chrome::current() {
                        chrome.bind_toolbar(toolbar)
                    } else {
                        toolbar
                    }
                }),
        )
        .child(
            view()
                .absolute()
                .left_3()
                .right_3()
                .z_index(10)
                .bottom(move || {
                    px(if compact() {
                        40.0
                    } else if toolbar_wrapped.get() {
                        56.0 + super::DESKTOP_TOOLBAR_EXTRA_HEIGHT
                    } else {
                        56.0
                    })
                })
                .style_when(
                    move || !is_desktop() && compact(),
                    |style| style.left_4().right_4().bottom_20(),
                )
                .child(dynamic(move || {
                    let trigger = ui::player_binge(move || {
                        format!(
                            "{} · {}  ›",
                            tr("player.watchFullSeries"),
                            tr("player.totalEpisodes")
                                .replace("{count}", &state.episode_count.get().to_string())
                        )
                    })
                    .style_when(
                        move || {
                            let available = state.can_binge && state.episode_count.get() > 0;
                            !available || state.in_binge.get() || super::landscape::active()
                        },
                        |style| style.hidden(),
                    )
                    .disabled(state.busy)
                    .on_press(move || action.with(|action| action(ControlAction::EnterBinge)));
                    view().child(trigger)
                })),
        )
        .child(when(compact, move || {
            menus::episode_sheet(state.menus, state.busy, more_stop, move |intent| {
                action.with(|action| action(intent.into()))
            })
        }))
}

fn enhancement_control(available: Memo<bool>) -> View {
    let trigger = ui::player_icon_button(Icon::New, move || tr("player.enhanceSettings"))
        .disabled(move || !available.get());
    let Some(state) = crate::player::enhancement::Controller::current() else {
        return view().child(trigger);
    };
    ui::player_enhancement_panel(
        state.open,
        move |open| state.open.set(open),
        trigger.selected(move || {
            state.open.get() || state.settings.get().anime4k_on || state.settings.get().sharpen_on
        }),
        move || ui::enhancement_settings(state.settings, state.phase, state.message),
    )
}

fn labelled(glyph: Icon, label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    ui::player_control("")
        .flex_row()
        .gap_1p5()
        .child(ui::player_hint(move || match glyph {
            Icon::Speed => tr("player.playbackRate"),
            Icon::Quality => tr("player.definition"),
            Icon::Episodes => tr("player.episodes"),
            _ => label.get(),
        }))
        .child(ui::icon(glyph))
        .child(
            text(move || label.get())
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::NORMAL),
        )
}
impl From<MenuAction> for ControlAction {
    fn from(action: MenuAction) -> Self {
        match action {
            MenuAction::SetRate(rate) => Self::SetRate(rate),
            MenuAction::SetDefinition(definition) => Self::SetDefinition(definition),
            MenuAction::SelectEpisode(episode) => Self::SelectEpisode(episode),
            MenuAction::SetVolume(volume) => Self::SetVolume(volume),
            MenuAction::ToggleMute => Self::ToggleMute,
            MenuAction::SwitchSeason(id) => Self::SwitchSeason(id),
            MenuAction::ReserveSeason(id) => Self::ReserveSeason(id),
            MenuAction::RetrySeasons => Self::RetrySeasons,
        }
    }
}
