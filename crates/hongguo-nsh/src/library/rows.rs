use super::{
    Kind,
    date::{date_label, publish_time},
    model::Entry,
};
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct Rows {
    pub(super) kind: Kind,
    pub(super) nav: Navigation,
    pub(super) items: Memo<Vec<Entry>>,
    pub(super) busy: Signal<bool>,
    pub(super) managing: Signal<bool>,
    pub(super) selected: Signal<Vec<String>>,
    pub(super) has_more: Memo<bool>,
    pub(super) scroll_scope: Memo<(u64, bool, String, i64)>,
}

pub(super) fn build(
    rows: Rows,
    remove: impl Fn(Vec<Entry>) + Copy + 'static,
    play: impl Fn(Entry) + Copy + 'static,
    more: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    if matches!(rows.kind, Kind::Collections | Kind::Liked) {
        return super::posters::build(rows, remove, play).into_element();
    }
    let footer = move || {
        rows.kind == Kind::Follows && !rows.has_more.get() && !rows.items.with(Vec::is_empty)
    };
    crate::view_memory::remember_scroll(
        virtual_list(
            move || rows.items.with(Vec::len),
            move |at| {
                let item =
                    computed(move || rows.items.with(|items| items.get(at.index()).cloned()));
                let select = move || {
                    let Some(item) = item.get_untracked() else {
                        return;
                    };
                    if rows.managing.get_untracked() {
                        rows.selected.update(|keys| {
                            if keys.contains(&item.key) {
                                keys.retain(|key| key != &item.key);
                            } else {
                                keys.push(item.key);
                            }
                        });
                    } else {
                        rows.nav.push(Route::Detail(item.series_id));
                    }
                };
                view()
                    .pb(move || {
                        px(if at.index() + 1 < rows.items.with(Vec::len) {
                            8.0
                        } else {
                            0.0
                        })
                    })
                    .child(
                        ui::personal_card(move || {
                            item.get().is_some_and(|item| {
                                rows.selected.with(|keys| keys.contains(&item.key))
                            })
                        })
                        .child(when(rows.managing, move || {
                            ui::personal_pick(
                                move || {
                                    item.get()
                                        .map_or(SharedString::default(), |item| item.title.into())
                                },
                                move || {
                                    item.get().is_some_and(|item| {
                                        rows.selected.with(|keys| keys.contains(&item.key))
                                    })
                                },
                            )
                            .disabled(rows.busy)
                            .on_change(move |_: &bool| select())
                        }))
                        .child(
                            ui::personal_cover(move || {
                                item.get().map(|item| item.cover).unwrap_or_default()
                            })
                            .accessibility_label(move || {
                                item.get().map(|item| SharedString::from(item.title))
                            })
                            .disabled(rows.busy)
                            .on_press(select)
                            .child(when(
                                move || {
                                    item.get()
                                        .and_then(|item| item.reservation)
                                        .is_some_and(|item| item.publish_time > 0)
                                },
                                move || {
                                    ui::personal_date(move || {
                                        item.get()
                                            .and_then(|item| item.reservation)
                                            .map_or(SharedString::default(), |item| {
                                                online_date(&item)
                                            })
                                    })
                                },
                            )),
                        )
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .justify_center()
                                .gap_1()
                                .child(
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            ui::personal_title(move || {
                                                item.get().map_or(SharedString::default(), |item| {
                                                    item.title.into()
                                                })
                                            })
                                            .disabled(rows.busy)
                                            .on_press(select),
                                        )
                                        .child(when(
                                            move || {
                                                item.get()
                                                    .and_then(|item| item.reservation)
                                                    .is_some_and(|item| item.score > 0.0)
                                            },
                                            move || {
                                                ui::personal_score(move || {
                                                    item.get()
                                                        .and_then(|item| item.reservation)
                                                        .map_or(0.0, |item| item.score)
                                                })
                                            },
                                        )),
                                )
                                .child(
                                    text(move || {
                                        item.get()
                                            .map_or(SharedString::default(), |item| status(&item))
                                    })
                                    .text_xs()
                                    .line_height(px(16.0))
                                    .text_color(move || theme().muted_foreground)
                                    .number_of_lines(1),
                                )
                                .child(when(
                                    move || rows.kind == Kind::Follows,
                                    move || {
                                        text(move || {
                                            item.get().map_or(SharedString::default(), |item| {
                                                tr("follow.episodeProgress")
                                                    .replace(
                                                        "{watched}",
                                                        &item.watched_episode.to_string(),
                                                    )
                                                    .replace(
                                                        "{total}",
                                                        &item.episode_count.to_string(),
                                                    )
                                                    .into()
                                            })
                                        })
                                        .text_xs()
                                        .line_height(px(16.0))
                                        .text_color(move || theme().muted_foreground)
                                    },
                                ))
                                .child(when(
                                    move || rows.kind == Kind::Reservations,
                                    move || {
                                        view()
                                            .mt_1()
                                            .flex_row()
                                            .flex_wrap()
                                            .items_center()
                                            .gap_1p5()
                                            .child(when(
                                                move || {
                                                    item.get()
                                                        .and_then(|item| item.reservation)
                                                        .is_some_and(|item| {
                                                            !item.category.is_empty()
                                                        })
                                                },
                                                move || {
                                                    ui::personal_badge(
                                                        move || {
                                                            item.get()
                                                                .and_then(|item| item.reservation)
                                                                .map_or(
                                                                    SharedString::default(),
                                                                    |item| item.category.into(),
                                                                )
                                                        },
                                                        true,
                                                    )
                                                },
                                            ))
                                            .child(each(
                                                move || {
                                                    item.get()
                                                        .and_then(|item| item.reservation)
                                                        .map(|item| {
                                                            item.rec_tags
                                                                .into_iter()
                                                                .filter(|tag| !tag.is_empty())
                                                                .take(3)
                                                                .collect::<Vec<_>>()
                                                        })
                                                        .unwrap_or_default()
                                                },
                                                |tag: &String| tag.clone(),
                                                move |tag, _| {
                                                    ui::personal_badge(
                                                        move || SharedString::from(tag.get()),
                                                        false,
                                                    )
                                                },
                                            ))
                                            .child(when(
                                                move || {
                                                    item.get()
                                                        .and_then(|item| item.reservation)
                                                        .is_some_and(|item| {
                                                            item.publish_time > 0 && !item.is_online
                                                        })
                                                },
                                                move || {
                                                    ui::personal_publish_at(move || {
                                                        item.get()
                                                            .and_then(|item| item.reservation)
                                                            .map_or_else(
                                                                SharedString::default,
                                                                |item| {
                                                                    tr("reservation.publishAt")
                                                                        .replace(
                                                                            "{time}",
                                                                            &publish_time(
                                                                                item.publish_time,
                                                                            ),
                                                                        )
                                                                        .into()
                                                                },
                                                            )
                                                    })
                                                },
                                            ))
                                    },
                                )),
                        )
                        .child(when(
                            move || !rows.managing.get(),
                            move || {
                                view()
                                    .flex_shrink_0()
                                    .justify_center()
                                    .items_stretch()
                                    .gap_1p5()
                                    .child(when(
                                        move || item.get().is_some_and(|item| item.can_play),
                                        move || {
                                            ui::library_action(
                                                move || tr("player.play"),
                                                Some(ui::Icon::Play),
                                            )
                                            .disabled(rows.busy)
                                            .on_press(
                                                move || {
                                                    if let Some(item) = item.get_untracked() {
                                                        play(item);
                                                    }
                                                },
                                            )
                                        },
                                    ))
                                    .child(when(
                                        move || {
                                            rows.kind == Kind::Follows
                                                && item.get().is_some_and(|item| {
                                                    item.finished == Some(false)
                                                })
                                        },
                                        move || {
                                            ui::library_action(
                                                move || tr("follow.following"),
                                                Some(ui::Icon::Follows),
                                            )
                                            .disabled(rows.busy)
                                            .on_press(
                                                move || {
                                                    if let Some(item) = item.get_untracked() {
                                                        remove(vec![item]);
                                                    }
                                                },
                                            )
                                        },
                                    ))
                                    .child(when(
                                        move || {
                                            rows.kind == Kind::Reservations
                                                && item.get().is_some_and(|item| item.can_play)
                                        },
                                        move || {
                                            ui::personal_detail(move || tr("rank.detail"))
                                                .disabled(rows.busy)
                                                .on_press(select)
                                        },
                                    ))
                                    .child(when(
                                        move || {
                                            rows.kind == Kind::Reservations
                                                && item.get().is_some_and(|item| !item.can_play)
                                        },
                                        move || {
                                            ui::library_action(
                                                move || tr("reservation.cancel"),
                                                Some(ui::Icon::Reservations),
                                            )
                                            .disabled(rows.busy)
                                            .on_press(
                                                move || {
                                                    if let Some(item) = item.get_untracked() {
                                                        remove(vec![item]);
                                                    }
                                                },
                                            )
                                        },
                                    ))
                            },
                        )),
                    )
                    .into_element()
            },
        ),
        rows.kind.memory_name(),
        rows.scroll_scope,
    )
    .flex_1()
    .on_load_more(move || {
        if rows.kind == Kind::Reservations {
            more();
        }
    })
    .max_h(move || {
        let count = rows.items.with(Vec::len);
        let cards = count as f32 * (92.0 * 4.0 / 3.0 + 26.0) + count.saturating_sub(1) as f32 * 8.0;
        px(cards
            + if footer() {
                32.0
            } else if rows.kind == Kind::Reservations {
                17.0
            } else {
                0.0
            })
    })
    .keys(move || {
        rows.items
            .get()
            .into_iter()
            .map(|item| item.key)
            .collect::<Vec<_>>()
    })
    .footer(
        view()
            .child(when(footer, move || {
                view().pt_4().items_center().child(
                    text(move || tr("follow.noMore"))
                        .text_xs()
                        .line_height(px(16.0))
                        .text_color(move || theme().muted_foreground),
                )
            }))
            .child(when(
                move || rows.kind == Kind::Reservations,
                move || view().h(px(17.0)),
            )),
    )
    .estimated_item_size(px(92.0 * 4.0 / 3.0 + 34.0))
    .into_element()
}

fn status(entry: &Entry) -> SharedString {
    let Some(finished) = entry.finished else {
        return entry.summary.clone().into();
    };
    let serial = tr(if finished {
        "follow.finished"
    } else {
        "follow.serializing"
    });
    if entry.watched_episode <= 0 {
        return serial;
    }
    let remaining = entry.episode_count - entry.watched_episode;
    let progress = if remaining <= 0 {
        tr("follow.caughtUp")
    } else {
        tr("follow.unwatched")
            .replace("{count}", &remaining.to_string())
            .into()
    };
    format!("{progress} · {serial}").into()
}

fn online_date(item: &hongguo_api::calendar::CalendarItem) -> SharedString {
    let date = date_label(item.publish_time, crate::i18n::is_chinese());
    tr(if item.is_online {
        "reservation.onlineBadge"
    } else {
        "reservation.upcomingBadge"
    })
    .replace("{date}", &date)
    .into()
}
