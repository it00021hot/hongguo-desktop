use super::menu_model::RATES;
use crate::{i18n::tr, ui};
use hongguo_common::model::VideoDefinition;
use nsh_core::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Rate,
    Quality,
    Episodes,
    Volume,
}

#[derive(Clone, Copy)]
pub(crate) struct MenuState {
    pub(crate) open: Signal<Option<Panel>>,
    pub(crate) definitions: Memo<Vec<VideoDefinition>>,
    pub(crate) selected_definition: Memo<Option<u32>>,
    pub(crate) episodes: Memo<Vec<u32>>,
    pub(crate) current_episode: Memo<u32>,
    pub(crate) volume: Signal<f32>,
    pub(crate) muted: Signal<bool>,
    pub(crate) seasons: SeasonState,
}

#[derive(Clone, Default, PartialEq)]
pub(crate) struct Season {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) cover: String,
    pub(crate) tag: String,
    pub(crate) episodes: u32,
    pub(crate) play_count: i64,
    pub(crate) current: bool,
    pub(crate) reserved: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct SeasonState {
    pub(crate) items: Memo<Vec<Season>>,
    pub(crate) loading: Memo<bool>,
    pub(crate) error: Memo<SharedString>,
    pub(crate) reserving: Memo<bool>,
    pub(crate) downloaded: Memo<Vec<u32>>,
}

#[cfg(test)]
impl SeasonState {
    pub(super) fn empty() -> Self {
        Self {
            items: computed(Vec::new),
            loading: computed(|| false),
            error: computed(SharedString::default),
            reserving: computed(|| false),
            downloaded: computed(Vec::new),
        }
    }
}

impl MenuState {
    pub(super) fn set_open(self, panel: Panel, open: bool) {
        if open {
            self.open.set(Some(panel));
        } else if self.open.get_untracked() == Some(panel) {
            self.open.set(None);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    SetRate(f32),
    SetDefinition(Option<u32>),
    SelectEpisode(u32),
    SetVolume(f32),
    ToggleMute,
    SwitchSeason(String),
    ReserveSeason(String),
    RetrySeasons,
}

pub(crate) fn rate(
    state: MenuState,
    rate: Signal<f32>,
    busy: Memo<bool>,
    trigger: nsh_base::ButtonElement,
    action: impl Fn(MenuAction) + Copy + 'static,
) -> impl IntoElement {
    ui::player_menu(
        move || state.open.get() == Some(Panel::Rate),
        move |open| state.set_open(Panel::Rate, open),
        trigger.disabled(move || busy.get()),
        move || {
            view()
                .child(ui::player_menu_label(move || tr("player.playbackRate")))
                .children(RATES.into_iter().map(move |value| {
                    ui::player_menu_option(format!("{value}x"), move || rate.get() == value)
                        .child(
                            text(format!("{value}x"))
                                .text_sm()
                                .font_family(move || {
                                    nsh_base::theme()
                                        .font_mono
                                        .clone()
                                        .unwrap_or_else(|| "monospace".into())
                                })
                                .font_weight(FontWeight::NORMAL),
                        )
                        .disabled(move || busy.get())
                        .on_select(move || action(MenuAction::SetRate(value)))
                }))
        },
    )
}

pub(crate) fn quality(
    state: MenuState,
    busy: Memo<bool>,
    trigger: nsh_base::ButtonElement,
    action: impl Fn(MenuAction) + Copy + 'static,
) -> impl IntoElement {
    ui::player_menu(
        move || state.open.get() == Some(Panel::Quality),
        move |open| state.set_open(Panel::Quality, open),
        trigger.disabled(move || busy.get() || state.definitions.with(Vec::is_empty)),
        move || {
            view()
                .child(ui::player_menu_label(move || tr("player.definition")))
                .child(
                    ui::player_menu_option(
                        move || tr("player.definitionAuto"),
                        move || state.selected_definition.get().is_none(),
                    )
                    .child(
                        text(move || tr("player.definitionAuto"))
                            .text_sm()
                            .font_weight(FontWeight::NORMAL),
                    )
                    .disabled(move || busy.get())
                    .on_select(move || action(MenuAction::SetDefinition(None))),
                )
                .child(each(
                    move || state.definitions.get(),
                    |definition: &VideoDefinition| definition.value,
                    move |definition, _| {
                        ui::player_menu_option(
                            move || format!("{}P", definition.get().value),
                            move || state.selected_definition.get() == Some(definition.get().value),
                        )
                        .child(
                            text(move || format!("{}P", definition.get().value))
                                .text_sm()
                                .font_family(move || {
                                    nsh_base::theme()
                                        .font_mono
                                        .clone()
                                        .unwrap_or_else(|| "monospace".into())
                                })
                                .font_weight(FontWeight::NORMAL),
                        )
                        .disabled(move || busy.get())
                        .on_select(move || {
                            action(MenuAction::SetDefinition(Some(
                                definition.get_untracked().value,
                            )))
                        })
                    },
                ))
        },
    )
}

pub(crate) fn episodes(
    state: MenuState,
    busy: Memo<bool>,
    trigger: nsh_base::ButtonElement,
    action: impl Fn(MenuAction) + Copy + 'static,
) -> View {
    ui::player_episode_panel(
        move || state.open.get() == Some(Panel::Episodes),
        move |open| state.set_open(Panel::Episodes, open),
        trigger.disabled(move || busy.get() || state.episodes.with(Vec::is_empty)),
        move || super::episodes::picker(state, busy, action),
    )
}

pub(super) fn episode_sheet(
    state: MenuState,
    busy: Memo<bool>,
    return_focus: ElementRef,
    action: impl Fn(MenuAction) + Copy + 'static,
) -> View {
    ui::player_episode_sheet(
        move || state.open.get() == Some(Panel::Episodes),
        move |open| state.set_open(Panel::Episodes, open),
        return_focus,
        move || super::episodes::picker(state, busy, action),
    )
}

pub(crate) fn volume(
    state: MenuState,
    trigger: nsh_base::ButtonElement,
    action: impl Fn(MenuAction) + Copy + 'static,
) -> View {
    ui::player_volume_panel(
        move || state.open.get() == Some(Panel::Volume),
        move |open| state.set_open(Panel::Volume, open),
        trigger,
        move || {
            view()
                .test_id("player-volume-panel")
                .items_center()
                .gap_2()
                .child(
                    text(move || {
                        format!(
                            "{}",
                            (if state.muted.get() {
                                0.0
                            } else {
                                state.volume.get()
                            } * 100.0)
                                .round() as u32
                        )
                    })
                    .text_xs()
                    .font_weight(FontWeight::NORMAL),
                )
                .child(
                    ui::player_volume_slider(move || {
                        if state.muted.get() {
                            0.0
                        } else {
                            state.volume.get()
                        }
                    })
                    .on_change(move |value: &nsh_base::SliderValue| {
                        action(MenuAction::SetVolume(value.start() as f32))
                    }),
                )
                .child(dynamic(move || {
                    ui::player_icon_button(
                        if state.muted.get() || state.volume.get() <= 0.0 {
                            ui::Icon::Muted
                        } else {
                            ui::Icon::Volume
                        },
                        move || {
                            tr(if state.muted.get() || state.volume.get() <= 0.0 {
                                "player.unmute"
                            } else {
                                "player.mute"
                            })
                        },
                    )
                    .on_press(move || action(MenuAction::ToggleMute))
                }))
        },
    )
}
