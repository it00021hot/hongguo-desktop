use super::{ControlAction, ControlState, menus};
use crate::{i18n::tr, ui, window_modes::WindowModes};
use nsh_core::prelude::*;

pub(super) fn active() -> bool {
    WindowModes::current().is_some_and(|modes| modes.state().landscape)
}

pub(super) fn row(
    state: ControlState,
    action: impl Fn(ControlAction) + Copy + 'static,
    composer: impl Fn(Memo<ElementRef>) -> AnyElement + 'static,
) -> View {
    let danmaku = element_ref();
    let episodes = element_ref();
    ui::player_landscape_row()
        .child(
            ui::player_landscape_danmaku(state.danmaku_enabled)
                .element_ref(&danmaku)
                .on_press(move || action(ControlAction::Danmaku)),
        )
        .child(
            view()
                .flex_1()
                .min_w_0()
                .child(composer(computed(move || danmaku))),
        )
        .child(
            ui::player_landscape_label(move || format!("{}x", state.rate.get()))
                .accessibility_label(move || Some(tr("player.playbackRate")))
                .disabled(state.busy)
                .on_press(move || {
                    let rates = super::menu_model::RATES;
                    let index = rates
                        .iter()
                        .position(|rate| (*rate - state.rate.get_untracked()).abs() < 0.01);
                    action(ControlAction::SetRate(
                        rates[index.map_or(0, |index| (index + 1) % rates.len())],
                    ));
                }),
        )
        .child(
            ui::player_landscape_label(move || {
                if state.menus.selected_definition.get().is_none() {
                    tr("player.definitionAuto")
                } else {
                    state.quality.get()
                }
            })
            .accessibility_label(move || Some(tr("player.definition")))
            .disabled(move || state.busy.get() || state.menus.definitions.with(Vec::is_empty))
            .on_press(move || {
                let values = std::iter::once(None)
                    .chain(
                        state
                            .menus
                            .definitions
                            .get_untracked()
                            .into_iter()
                            .map(|item| Some(item.value)),
                    )
                    .collect::<Vec<_>>();
                let current = state.menus.selected_definition.get_untracked();
                let next = values
                    .iter()
                    .position(|value| *value == current)
                    .map_or(0, |index| (index + 1) % values.len());
                action(ControlAction::SetDefinition(values[next]));
            }),
        )
        .child(
            ui::player_landscape_label(move || {
                tr("player.episodesBar").replace("{count}", &state.episode_count.get().to_string())
            })
            .element_ref(&episodes)
            .disabled(state.busy)
            .accessibility_label(move || Some(tr("player.episodes")))
            .on_press(move || state.menus.open.set(Some(super::Panel::Episodes))),
        )
        .child(menus::episode_sheet(
            state.menus,
            state.busy,
            episodes,
            move |intent| action(intent.into()),
        ))
}
