use super::{actions::Reservations, date};
use crate::{bridge, i18n::tr, ui};
use hongguo_api::calendar::{CalendarItem, CalendarPage};
use nsh_core::prelude::*;

pub(super) type Feed = Resource<CalendarPage, bridge::Failure>;

pub(super) struct State {
    pub(super) feed: Feed,
    pub(super) selected: Signal<String>,
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
        reservations,
        today,
    } = state;
    let today = stored(today);
    let events = stored(events);
    view()
        .flex_1()
        .min_h(px(0.0))
        .gap_4()
        .child(view().px_4().child(ui::calendar_dates(
            move || feed.with(|page| page.map_or(0, |page| page.dates.len())),
            each(
                move || feed.with(|page| page.map(|page| page.dates.clone()).unwrap_or_default()),
                |date: &String| date.clone(),
                move |day, _| {
                    ui::calendar_date(
                        move || {
                            date::weekday_key(&day.get(), &today.get())
                                .map_or_else(SharedString::default, tr)
                        },
                        move || SharedString::from(date::day_month(&day.get())),
                        move || {
                            let selected = selected.get();
                            if selected.is_empty() {
                                feed.with(|page| {
                                    page.is_some_and(|page| page.default_date == day.get())
                                })
                            } else {
                                selected == day.get()
                            }
                        },
                    )
                    .on_change(move |checked: &bool| {
                        if *checked {
                            selected.set(day.get_untracked());
                        }
                    })
                },
            ),
        )))
        .child(ui::status(reservations.message))
        .child(when(
            move || feed.error().is_some(),
            move || {
                crate::pages::error_state(
                    move || {
                        feed.error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || feed.refetch(),
                )
            },
        ))
        .child(dynamic(move || {
            selected.get();
            ui::refresh_shade(move || {
                feed.loading() && feed.with(|page| page.is_some_and(|page| !page.items.is_empty()))
            })
            .style_when(move || feed.error().is_some(), |style| style.hidden())
            .child(
                virtual_list(
                    move || feed.with(|page| page.map_or(0, |page| page.items.len())),
                    move |at| {
                        let item = computed(move || {
                            feed.with(|page| {
                                page.and_then(|page| page.items.get(at.index())).cloned()
                            })
                        });
                        ui::calendar_row(
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
                .test_id("new-calendar-list")
                .flex_1()
                .content_padding_x(px(16.0))
                .row_gap(px(8.0))
                .estimated_item_size(px(133.0))
                .keys(move || {
                    feed.with(|page| {
                        page.map(|page| {
                            page.items
                                .iter()
                                .map(|item| item.series_id.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                    })
                })
                .on_load_more(move || events.with(|events| (events.more)()))
                .empty(view().child(dynamic(move || {
                    if feed.loading() {
                        return view().child(ui::status(move || tr("common.loading")));
                    }
                    view().py_16().items_center().child(
                        text(move || tr("newDrama.empty"))
                            .text_sm()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(move || nsh_base::theme().muted_foreground),
                    )
                })))
                .footer(view().pt_4().pb_2().items_center().child(when(
                    move || feed.with(|page| page.is_some_and(|page| !page.items.is_empty())),
                    move || {
                        text(move || {
                            tr(if feed.loading() {
                                "feed.loadingMore"
                            } else {
                                "feed.end"
                            })
                        })
                        .style_when(
                            move || {
                                feed.with(|page| page.is_some_and(|page| page.has_more))
                                    && !feed.loading()
                            },
                            |style| style.hidden(),
                        )
                        .text_sm()
                        .line_height(px(20.0))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(move || nsh_base::theme().muted_foreground)
                    },
                ))),
            )
        }))
}

fn row_data(item: &CalendarItem, reservations: Reservations) -> ui::CalendarRowData {
    ui::CalendarRowData {
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
