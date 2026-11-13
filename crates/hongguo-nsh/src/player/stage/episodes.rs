use super::{
    menu_model::{GROUP_SIZE, adjacent, group_for, group_label, group_slice},
    menus::{MenuAction, MenuState, Panel, Season},
};
use crate::{i18n::tr, ui};
use nsh_core::{element::ImageSrc, prelude::*};

pub(super) fn picker(
    state: MenuState,
    busy: Memo<bool>,
    action: impl Fn(MenuAction) + Copy + 'static,
) -> View {
    let series = signal(false);
    view()
        .test_id("player-episode-panel")
        .gap_3()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    view()
                        .flex_row()
                        .items_end()
                        .gap_4()
                        .child(
                            ui::picker_tab(
                                move || tr("player.episodes"),
                                move || !series.get(),
                                0_usize,
                            )
                            .on_press(move || series.set(false)),
                        )
                        .child(
                            ui::picker_tab(
                                move || tr("player.seriesList"),
                                series,
                                move || state.seasons.items.with(Vec::len),
                            )
                            .on_press(move || series.set(true)),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(when(
                            move || !series.get() && state.episodes.with(|items| !items.is_empty()),
                            move || {
                                text(move || {
                                    tr("player.totalEpisodes").replace(
                                        "{count}",
                                        &state.episodes.with(Vec::len).to_string(),
                                    )
                                })
                                .text_xs()
                                .line_height(px(16.0))
                                .text_color(move || ui::picker_color("video-picker-muted"))
                            },
                        ))
                        .child(when(is_desktop, move || {
                            ui::picker_close(move || tr("common.close"))
                                .on_press(move || state.open.set(None))
                        })),
                ),
        )
        .child(dynamic(move || {
            if series.get() {
                season_list(state, action)
            } else {
                grid(state, busy, action)
            }
        }))
}

fn grid(state: MenuState, busy: Memo<bool>, action: impl Fn(MenuAction) + Copy + 'static) -> View {
    let width = signal(0.0_f32);
    let columns = computed(move || {
        if is_desktop() {
            ui::PICKER_DESKTOP_COLUMNS
        } else {
            ui::picker_mobile_columns(width.get())
        }
    });
    effect(move || {
        if state.open.get() != Some(Panel::Episodes) {
            return;
        }
        let stride = columns.get() as isize;
        for (key, offset) in [
            ("left", -1_isize),
            ("right", 1),
            ("up", -stride),
            ("down", stride),
        ] {
            on_key_with(
                key,
                KeyOptions {
                    when: nsh_core::keymap::When::NotEditing,
                    repeat: true,
                },
                move |_| {
                    if busy.get_untracked() {
                        return;
                    }
                    if let Some(episode) = state.episodes.with_untracked(|episodes| {
                        adjacent(episodes, state.current_episode.get_untracked(), offset)
                    }) {
                        action(MenuAction::SelectEpisode(episode));
                    }
                },
            );
        }
    });
    let group = signal(group_for(
        &state.episodes.get_untracked(),
        state.current_episode.get_untracked(),
    ));
    effect(move || {
        group.set(group_for(
            &state.episodes.get(),
            state.current_episode.get(),
        ))
    });
    let slice = computed(move || {
        state
            .episodes
            .with(|episodes| episodes[group_slice(episodes.len(), group.get())].to_vec())
    });
    view()
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .gap_3()
        .child(when(
            move || state.episodes.with(Vec::len) > GROUP_SIZE,
            move || {
                ui::player_episode_group()
                    .flex_row()
                    .flex_wrap()
                    .gap_x_4()
                    .gap_y_1()
                    .child(each(
                        move || {
                            (0..state
                                .episodes
                                .with(|episodes| episodes.len().div_ceil(GROUP_SIZE)))
                                .collect::<Vec<_>>()
                        },
                        |group: &usize| *group,
                        move |index, _| {
                            ui::player_episode(
                                move || {
                                    state
                                        .episodes
                                        .with(|episodes| group_label(episodes, index.get()))
                                },
                                move || group.get() == index.get(),
                            )
                            .child(
                                text(move || {
                                    state
                                        .episodes
                                        .with(|episodes| group_label(episodes, index.get()))
                                })
                                .text_size(px(13.0))
                                .line_height(px(20.0)),
                            )
                            .on_change(move |selected: &bool| {
                                if *selected {
                                    group.set(index.get_untracked());
                                }
                            })
                        },
                    ))
            },
        ))
        .child(dynamic(move || {
            virtual_list(
                move || slice.with(Vec::len),
                move |at| {
                    let episode =
                        computed(move || slice.with(|episodes| episodes.get(at.index()).copied()));
                    let active =
                        computed(move || episode.get() == Some(state.current_episode.get()));
                    ui::player_episode_cell(
                        move || {
                            tr("player.episodeNo")
                                .replace("{index}", &episode.get().unwrap_or_default().to_string())
                        },
                        active,
                    )
                    .relative()
                    .disabled(move || busy.get() || episode.get().is_none())
                    .child(
                        text(move || episode.get().unwrap_or_default().to_string())
                            .text_sm()
                            .line_height(px(20.0)),
                    )
                    .child(when(
                        move || {
                            episode.get().is_some_and(|episode| {
                                state
                                    .seasons
                                    .downloaded
                                    .with(|done| done.contains(&episode))
                            })
                        },
                        move || ui::picker_downloaded(active),
                    ))
                    .on_press(move || {
                        if let Some(episode) = episode.get_untracked() {
                            action(MenuAction::SelectEpisode(episode));
                        }
                    })
                },
            )
            .test_id("player-episode-grid")
            .h(move || {
                let rows = slice.with(Vec::len).div_ceil(columns.get() as usize);
                px(rows as f32 * if is_desktop() { 36.0 } else { 48.0 }
                    + rows.saturating_sub(1) as f32 * ui::PICKER_GRID_GAP)
            })
            .columns(move || columns.get())
            .row_gap(px(ui::PICKER_GRID_GAP))
            .column_gap(px(ui::PICKER_GRID_GAP))
            .estimated_item_size(px(if is_desktop() { 36.0 } else { 48.0 }))
            .keys(move || slice.get())
        }))
}

fn season_list(state: MenuState, action: impl Fn(MenuAction) + Copy + 'static) -> View {
    view()
        .gap_2()
        .child(when(
            move || state.seasons.loading.get(),
            move || ui::status(move || tr("common.loading")),
        ))
        .child(when(
            move || !state.seasons.error.get().is_empty(),
            move || {
                view().gap_2().child(ui::status(state.seasons.error)).child(
                    ui::player_control(move || tr("common.retry"))
                        .disabled(move || state.seasons.reserving.get())
                        .on_press(move || action(MenuAction::RetrySeasons)),
                )
            },
        ))
        .child(dynamic(move || {
            virtual_list(
                move || state.seasons.items.with(Vec::len),
                move |at| {
                    let season = computed(move || {
                        state
                            .seasons
                            .items
                            .with(|items| items.get(at.index()).cloned())
                    });
                    dynamic(move || {
                        season
                            .get()
                            .map(|season| season_card(state, season, action))
                    })
                },
            )
            .test_id("player-season-list")
            .h(move || {
                px((state.seasons.items.with(Vec::len) as f32 * 78.0
                    + state.seasons.items.with(Vec::len).saturating_sub(1) as f32 * 8.0)
                    .min((window().height.0 * 0.62 - 72.0).max(78.0)))
            })
            .estimated_item_size(px(78.0))
            .row_gap(px(8.0))
            .keys(move || {
                state
                    .seasons
                    .items
                    .with(|items| items.iter().map(|item| item.id.clone()).collect::<Vec<_>>())
            })
        }))
}

fn season_card(
    state: MenuState,
    season: Season,
    action: impl Fn(MenuAction) + Copy + 'static,
) -> View {
    let current = season.current;
    let unreleased = season.episodes == 0;
    let stored = stored(season);
    let row = ui::picker_season(current, unreleased)
        .child(
            view()
                .w(px(40.0))
                .h(px(56.0))
                .flex_shrink_0()
                .rounded(px(8.0))
                .overflow_hidden()
                .child(
                    image(move || ImageSrc::from(stored.with(|season| season.cover.clone())))
                        .size_full()
                        .object_fit(nsh_core::ResizeMode::Cover),
                ),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap_0p5()
                .child(
                    text(move || stored.with(|season| season.title.clone()))
                        .text_sm()
                        .line_height(px(20.0))
                        .font_weight(FontWeight::MEDIUM)
                        .number_of_lines(1),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(when(
                            move || unreleased,
                            || {
                                view()
                                    .px_1()
                                    .rounded(px(2.0))
                                    .bg(move || {
                                        ui::picker_color("video-picker-active").opacity(0.9)
                                    })
                                    .child(
                                        text(move || tr("player.comingSoonBadge"))
                                            .text_size(px(10.0))
                                            .line_height(px(16.0)),
                                    )
                            },
                        ))
                        .child(
                            text(move || stored.with(|season| season_caption(season)))
                                .text_size(px(11.0))
                                .line_height(px(16.0))
                                .number_of_lines(1)
                                .text_color(move || ui::picker_color("video-picker-muted")),
                        ),
                ),
        );
    if current {
        return row.child(
            text(move || tr("player.nowPlaying"))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || ui::picker_color("video-picker-active")),
        );
    }
    if unreleased {
        return row.child(
            ui::picker_reserve(
                move || {
                    tr(if stored.with(|season| season.reserved) {
                        "player.reserved"
                    } else {
                        "player.reserve"
                    })
                },
                move || stored.with(|season| season.reserved),
            )
            .accessibility_label(move || {
                Some(
                    format!(
                        "{} · {}",
                        tr("player.reserve"),
                        stored.with(|season| season.title.clone())
                    )
                    .into(),
                )
            })
            .disabled(move || {
                state.seasons.reserving.get() || stored.with(|season| season.reserved)
            })
            .on_press(move || {
                action(MenuAction::ReserveSeason(
                    stored.with(|season| season.id.clone()),
                ))
            }),
        );
    }
    row.role(nsh_core::AccessibilityRole::Button)
        .focusable(move || !state.seasons.reserving.get() && !state.seasons.loading.get())
        .accessibility_state(move || nsh_core::AccessibilityState {
            disabled: state.seasons.reserving.get() || state.seasons.loading.get(),
            ..Default::default()
        })
        .label(move || stored.with(|season| season.title.clone()))
        .on_press(move || {
            if state.seasons.reserving.get_untracked() || state.seasons.loading.get_untracked() {
                return;
            }
            action(MenuAction::SwitchSeason(
                stored.with(|season| season.id.clone()),
            ))
        })
        .child(ui::picker_play(move || tr("player.play")))
}

fn season_caption(season: &Season) -> String {
    let mut label = season.tag.clone();
    if season.episodes > 0 {
        label.push_str(&format!(
            " · {}",
            tr("detail.episodesCount").replace("{count}", &season.episodes.to_string())
        ));
    }
    if season.play_count > 0 {
        label.push_str(&format!(
            " · {}{}",
            crate::i18n::format_play_count(season.play_count),
            tr("detail.plays")
        ));
    }
    label
}
