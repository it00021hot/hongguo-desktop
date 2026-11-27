use super::{FilteredPage, actions::Reservations, date};
use crate::{bridge, i18n::tr, ui};
use hongguo_service::contracts::calendar::{CalendarItem, CalendarPanel, SelectorId};
use nsh_core::prelude::*;

pub(super) type Feed = Resource<FilteredPage, bridge::Failure>;
pub(super) type Panel = Resource<(u64, CalendarPanel), bridge::Failure>;

pub(super) struct State {
    pub(super) feed: Feed,
    pub(super) selected: Signal<String>,
    pub(super) filter: Signal<Option<SelectorId>>,
    pub(super) panel: Panel,
    pub(super) identity: Memo<u64>,
    pub(super) fetching_more: Memo<bool>,
    pub(super) reservations: Reservations,
    pub(super) today: String,
}

pub(super) struct Events {
    pub(super) open: Box<dyn Fn(CalendarItem)>,
    pub(super) play: Box<dyn Fn(CalendarItem)>,
    pub(super) reserve: Box<dyn Fn(CalendarItem)>,
    pub(super) more: Box<dyn Fn()>,
}

pub(super) fn presentation(state: State, events: Events) -> View {
    let State {
        feed,
        selected,
        filter,
        panel,
        identity,
        fetching_more,
        reservations,
        today,
    } = state;
    let today = stored(today);
    let events = stored(events);
    let initial = computed(move || {
        feed.error().is_none()
            && (panel.loading() || feed.loading())
            && feed.with(|result| result.is_none_or(|result| result.page.items.is_empty()))
    });
    view()
        .flex_1()
        .min_h(px(0.0))
        .gap_4()
        .child(
            view()
                .px_4()
                .flex_row()
                .items_center()
                .gap_2()
                .child(genre_selector(panel, identity, filter))
                .child(view().min_w(px(0.0)).flex_1().child(
                    crate::features::new_drama::presentation::calendar_dates(
                        move || {
                            feed.with(|result| result.map_or(0, |result| result.page.dates.len()))
                        },
                        each(
                            move || {
                                feed.with(|page| {
                                    page.map(|result| result.page.dates.clone())
                                        .unwrap_or_default()
                                })
                            },
                            |date: &String| date.clone(),
                            move |day, _| {
                                crate::features::new_drama::presentation::calendar_date(
                                    move || {
                                        date::weekday_key(&day.get(), &today.get())
                                            .map_or_else(SharedString::default, tr)
                                    },
                                    move || SharedString::from(date::day_month(&day.get())),
                                    move || {
                                        let selected = selected.get();
                                        if selected.is_empty() {
                                            feed.with(|page| {
                                                page.is_some_and(|result| {
                                                    result.page.default_date == day.get()
                                                })
                                            })
                                        } else {
                                            selected == day.get()
                                        }
                                    },
                                )
                                .on_change(
                                    move |checked: &bool| {
                                        if *checked {
                                            selected.set(day.get_untracked());
                                        }
                                    },
                                )
                            },
                        ),
                    ),
                )),
        )
        .child(ui::status(reservations.message))
        .child(when(
            move || feed.error().is_some(),
            move || {
                crate::features::error_state(
                    move || {
                        feed.error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || feed.refetch(),
                )
            },
        ))
        .child(when(initial, move || {
            scroll()
                .flex_1()
                .child(crate::features::rank::presentation::rank_loading::loading_rows().px_4())
        }))
        .child(dynamic(move || {
            selected.get();
            filter.get();
            ui::refresh_shade(move || {
                feed.loading()
                    && !fetching_more.get()
                    && feed
                        .with(|result| result.is_some_and(|result| !result.page.items.is_empty()))
            })
            .style_when(
                move || initial.get() || feed.error().is_some(),
                |style| style.hidden(),
            )
            .child(
                virtual_list(
                    move || feed.with(|result| result.map_or(0, |result| result.page.items.len())),
                    move |at| {
                        let item = computed(move || {
                            feed.with(|page| {
                                page.and_then(|result| result.page.items.get(at.index()))
                                    .cloned()
                            })
                        });
                        crate::features::new_drama::presentation::calendar_row(
                            move || {
                                item.with(|item| {
                                    item.as_ref()
                                        .map(|item| row_data(item, reservations))
                                        .unwrap_or_default()
                                })
                            },
                            move || {
                                if let Some(item) = item.get_untracked() {
                                    events.with(|events| (events.open)(item));
                                }
                            },
                            move || {
                                if let Some(item) = item.get_untracked() {
                                    events.with(|events| (events.play)(item));
                                }
                            },
                            move || {
                                if let Some(item) = item.get_untracked() {
                                    events.with(|events| (events.reserve)(item));
                                }
                            },
                        )
                    },
                )
                .flex_1()
                .content_padding_x(px(16.0))
                .row_gap(px(8.0))
                .estimated_item_size(px(133.0))
                .keys(move || {
                    feed.with(|page| {
                        page.map(|result| {
                            result
                                .page
                                .items
                                .iter()
                                .map(|item| item.series_id.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                    })
                })
                .on_load_more(move || events.with(|events| (events.more)()))
                .empty(view().child(dynamic(move || {
                    crate::features::browse::presentation::browse_empty(move || {
                        tr(if filter.with(Option::is_none) {
                            "newDrama.empty"
                        } else {
                            "newDrama.calendarKind.noMatches"
                        })
                    })
                })))
                .footer(view().pt_4().pb_2().items_center().child(when(
                    move || {
                        feed.with(|result| {
                            result.is_some_and(|result| !result.page.items.is_empty())
                        })
                    },
                    move || {
                        text(move || {
                            tr(if fetching_more.get() {
                                "feed.loadingMore"
                            } else {
                                "feed.end"
                            })
                        })
                        .opacity(move || {
                            if fetching_more.get()
                                || (!feed.loading()
                                    && feed.with(|result| {
                                        result.is_some_and(|result| !result.page.has_more)
                                    }))
                            {
                                1.0
                            } else {
                                0.0_f32
                            }
                        })
                        .accessible(move || {
                            fetching_more.get()
                                || (!feed.loading()
                                    && feed.with(|result| {
                                        result.is_some_and(|result| !result.page.has_more)
                                    }))
                        })
                        .text_sm()
                        .line_height(px(20.0))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(move || nsh_base::theme().muted_foreground)
                    },
                ))),
            )
        }))
}

fn genre_selector(panel: Panel, identity: Memo<u64>, filter: Signal<Option<SelectorId>>) -> View {
    let row = computed(move || {
        panel.with(|panel| {
            panel
                .filter(|(owner, _)| *owner == identity.get())
                .and_then(|(_, panel)| panel.row.clone())
        })
    });
    view().child(when(
        move || row.with(|row| row.as_ref().is_some_and(|row| !row.options.is_empty())),
        move || {
            ui::select(
                move || tr("newDrama.calendarKind.all"),
                move || {
                    let mut options =
                        vec![ui::SelectOption::new("", tr("newDrama.calendarKind.all"))];
                    row.with(|row| {
                        if let Some(row) = row {
                            options.extend(row.options.iter().enumerate().map(
                                |(index, option)| {
                                    ui::SelectOption::new(index.to_string(), option.label.clone())
                                },
                            ));
                        }
                    });
                    options
                },
                move || {
                    let index = filter.with(|id| {
                        row.with(|row| {
                            row.as_ref()?
                                .options
                                .iter()
                                .position(|option| Some(&option.id) == id.as_ref())
                        })
                    });
                    Some(index.map_or_else(SharedString::default, |index| index.to_string().into()))
                },
            )
            .on_change(move |value: &Option<SharedString>| {
                let next = value
                    .as_deref()
                    .and_then(|value| value.parse::<usize>().ok())
                    .and_then(|index| {
                        row.with(|row| Some(row.as_ref()?.options.get(index)?.id.clone()))
                    });
                filter.set(next);
            })
        },
    ))
}

fn row_data(
    item: &CalendarItem,
    reservations: Reservations,
) -> crate::features::new_drama::presentation::CalendarRowData {
    crate::features::new_drama::presentation::CalendarRowData {
        title: item.title.clone(),
        cover: item.cover.clone(),
        online: item.is_online,
        heat: item.rec_tags.first().cloned().unwrap_or_default(),
        category: item.category.clone(),
        score: item.score,
        episodes: item.episode_cnt,
        description: item.description.clone(),
        published: date::publish_time(item.publish_time),
        reserved: item.has_subscribed
            || reservations
                .reserved
                .with(|items| items.contains(&item.series_id)),
        pending: reservations
            .pending
            .with(|items| items.contains(&item.series_id)),
    }
}
