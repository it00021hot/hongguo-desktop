use super::{
    SessionHandle,
    calendar::actions::Reservations,
    presentation::{CalendarRowData, calendar_row},
};
use crate::{
    bridge,
    i18n::tr,
    navigation::{DetailPrefill, Navigation},
    ui,
};
use hongguo_service::reservation_service::{
    self, SelectorRow, UpcomingAudience, UpcomingFilters, UpcomingItem, UpcomingPage,
};
use nsh_core::prelude::*;
use std::collections::HashSet;

#[derive(Clone, Default, PartialEq)]
struct Query {
    filters: UpcomingFilters,
    identity: u64,
    previous: Option<UpcomingPage>,
}

#[derive(Clone, PartialEq)]
struct Loaded {
    filters: UpcomingFilters,
    identity: u64,
    page: UpcomingPage,
}

pub(super) fn content(
    session: SessionHandle,
    nav: Navigation,
    active: Memo<bool>,
    reservations: Reservations,
) -> View {
    let query = signal(Query {
        identity: session.identity(),
        ..Default::default()
    });
    crate::view_memory::track_identity(
        move || session.identity(),
        query,
        |query, identity| {
            *query = Query {
                identity,
                ..Default::default()
            };
        },
    );
    let feed = resource(
        move || active.get().then(|| (query.get(), session.identity())),
        move |(mut query, identity), _| {
            if query.identity != identity {
                query = Query {
                    identity,
                    ..Default::default()
                };
            }
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let (offset, browse_session) = query.previous.as_ref().map_or((0, None), |page| {
                    (page.next_offset, Some(page.session_id.as_str()))
                });
                let page =
                    reservation_service::upcoming(&query.filters, offset, browse_session, &state)
                        .await?;
                Ok(Loaded {
                    filters: query.filters,
                    identity,
                    page: merge(query.previous, page),
                })
            })
        },
    );
    let selection = computed(move || query.with(|query| query.filters.clone()));
    let current = computed(move || {
        let identity = session.identity();
        let filters = selection.get();
        feed.with(|loaded| {
            loaded
                .filter(|loaded| loaded.filters == filters && loaded.identity == identity)
                .map(|loaded| loaded.page.clone())
        })
    });
    let display = computed(move || {
        current.get().or_else(|| {
            feed.loading()
                .then(|| {
                    let identity = session.identity();
                    feed.with(|loaded| {
                        loaded
                            .filter(|loaded| loaded.identity == identity)
                            .map(|loaded| loaded.page.clone())
                    })
                })
                .flatten()
        })
    });
    let initial = computed(move || {
        feed.loading()
            && feed.error().is_none()
            && display.with(|page| page.as_ref().is_none_or(|page| page.items.is_empty()))
    });
    let fetching_more =
        computed(move || feed.loading() && query.with(|query| query.previous.is_some()));
    let more = move || {
        if !active.get_untracked() || feed.loading() || feed.error().is_some() {
            return;
        }
        if let Some(page) = current.get_untracked().filter(|page| page.has_more) {
            query.update(|query| query.previous = Some(page));
        }
    };
    view()
        .flex_1()
        .min_h(px(0.0))
        .gap_3()
        .child(filters(session, active, query))
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
        .child(when(
            move || {
                !feed.loading()
                    && feed.error().is_none()
                    && current.with(|page| {
                        page.as_ref()
                            .is_some_and(|page| page.items.is_empty() && !page.has_more)
                    })
            },
            move || {
                crate::features::browse::presentation::browse_empty(move || {
                    tr("newDrama.upcomingPage.empty")
                })
            },
        ))
        .child(
            ui::refresh_shade(move || feed.loading() && !initial.get() && !fetching_more.get())
                .style_when(
                    move || {
                        initial.get()
                            || feed.error().is_some()
                            || display
                                .with(|page| page.as_ref().is_none_or(|page| page.items.is_empty()))
                    },
                    |style| style.hidden(),
                )
                .child(list(session, nav, selection, display, reservations, more)),
        )
        .child(when(
            move || {
                !feed.loading()
                    && feed.error().is_none()
                    && current.with(|page| {
                        page.as_ref()
                            .is_some_and(|page| page.items.is_empty() && page.has_more)
                    })
            },
            move || {
                view().px_4().child(
                    ui::button(move || tr("follow.loadMore"), ui::ButtonVariant::Secondary)
                        .on_press(more),
                )
            },
        ))
}

fn merge(previous: Option<UpcomingPage>, mut page: UpcomingPage) -> UpcomingPage {
    let mut previous = previous.unwrap_or_default();
    let mut seen: HashSet<_> = previous
        .items
        .iter()
        .map(|item| item.show.series_id.clone())
        .collect();
    previous.items.extend(
        page.items
            .into_iter()
            .filter(|item| seen.insert(item.show.series_id.clone())),
    );
    page.items = previous.items;
    page.has_more &= page.next_offset > previous.next_offset;
    if page.session_id.is_empty() {
        page.session_id = previous.session_id;
    }
    page
}

fn list(
    session: SessionHandle,
    nav: Navigation,
    selection: Memo<UpcomingFilters>,
    current: Memo<Option<UpcomingPage>>,
    reservations: Reservations,
    more: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    dynamic(move || {
        selection.get();
        virtual_list(
            move || current.with(|page| page.as_ref().map_or(0, |page| page.items.len())),
            move |at| {
                let item = computed(move || {
                    current.with(|page| {
                        page.as_ref()
                            .and_then(|page| page.items.get(at.index()))
                            .cloned()
                    })
                });
                let open = move || {
                    if let Some(item) = item.get_untracked() {
                        nav.detail(DetailPrefill::from_calendar(&item.show));
                    }
                };
                calendar_row(
                    move || {
                        item.with(|item| {
                            item.as_ref()
                                .map(|item| row(item, reservations))
                                .unwrap_or_default()
                        })
                    },
                    open,
                    open,
                    move || {
                        if let Some(item) = item.get_untracked() {
                            reservations.reserve(session, nav, item.show);
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
            current.with(|page| {
                page.as_ref()
                    .map(|page| {
                        page.items
                            .iter()
                            .map(|item| item.show.series_id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
        })
        .on_load_more(more)
    })
}

fn row(item: &UpcomingItem, reservations: Reservations) -> CalendarRowData {
    let show = &item.show;
    CalendarRowData {
        title: show.title.clone(),
        cover: show.cover.clone(),
        online: show.is_online,
        heat: item.anticipation.clone(),
        category: show.category.clone(),
        score: show.score,
        episodes: show.episode_cnt,
        description: show.description.clone(),
        published: (show.publish_time > 0)
            .then(|| chrono::DateTime::from_timestamp_millis(show.publish_time))
            .flatten()
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format("%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| item.expected_release.clone()),
        reserved: show.has_subscribed
            || reservations
                .reserved
                .with(|ids| ids.contains(&show.series_id)),
        pending: reservations
            .pending
            .with(|ids| ids.contains(&show.series_id)),
    }
}

fn filters(session: SessionHandle, active: Memo<bool>, query: Signal<Query>) -> impl IntoElement {
    let panel = resource(
        move || active.get().then(|| session.identity()),
        move |identity, _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let rows = reservation_service::upcoming_panel(&state).await?;
                Ok((identity, rows))
            })
        },
    );
    dynamic(move || {
        let identity = session.identity();
        let rows = panel.with(|result| {
            result
                .filter(|(owner, _)| *owner == identity)
                .map(|(_, rows)| rows.clone())
                .unwrap_or_default()
        });
        let root = view().px_4().gap_2();
        if rows.is_empty() {
            return if panel.loading() {
                root
            } else {
                root.child(legacy_audience(query))
            };
        }
        let panel_keys = stored(rows.iter().map(|row| row.key.clone()).collect::<Vec<_>>());
        root.children(
            rows.into_iter()
                .map(move |row| selector_row(query, row, panel_keys)),
        )
    })
}

fn legacy_audience(query: Signal<Query>) -> View {
    let options = [
        (UpcomingAudience::All, "newDrama.upcomingPage.audienceAll"),
        (UpcomingAudience::Male, "newDrama.upcomingPage.audienceMale"),
        (
            UpcomingAudience::Female,
            "newDrama.upcomingPage.audienceFemale",
        ),
    ];
    filter_row().children(options.into_iter().map(move |(value, label)| {
        crate::features::library::presentation::history_filter(
            move || tr(label),
            computed(move || query.with(|query| query.filters == UpcomingFilters::Legacy(value))),
        )
        .on_press(move || set_filters(query, UpcomingFilters::Legacy(value)))
    }))
}

fn selector_row(query: Signal<Query>, row: SelectorRow, panel_keys: Stored<Vec<String>>) -> View {
    let row_key = stored(row.key);
    let options = std::iter::once((None, row.label)).chain(
        row.options
            .into_iter()
            .map(|option| (Some(option.id), option.label)),
    );
    filter_row().children(options.map(move |(id, label)| {
        let id = stored(id);
        crate::features::library::presentation::history_filter(
            move || selector_label(&label),
            computed(move || {
                query.with(|query| {
                    let key = row_key.get();
                    let selected = match &query.filters {
                        UpcomingFilters::Legacy(_) => None,
                        UpcomingFilters::Selectors { selected, .. } => selected.get(&key),
                    };
                    id.with(|value| value.as_ref() == selected)
                })
            }),
        )
        .on_press(move || {
            let mut selected = query.with(|query| match &query.filters {
                UpcomingFilters::Selectors { selected, .. } => selected.clone(),
                UpcomingFilters::Legacy(_) => Default::default(),
            });
            let key = row_key.get();
            match id.get() {
                Some(id) => {
                    selected.insert(key, id);
                }
                None => {
                    selected.remove(&key);
                }
            }
            set_filters(
                query,
                UpcomingFilters::Selectors {
                    selected,
                    panel_keys: panel_keys.get(),
                },
            );
        })
    }))
}

fn set_filters(query: Signal<Query>, filters: UpcomingFilters) {
    query.update(|query| {
        if query.filters != filters {
            query.filters = filters;
            query.previous = None;
        }
    });
}

fn filter_row() -> View {
    view().flex_row().flex_wrap().items_center().gap_2()
}

fn selector_label(label: &str) -> SharedString {
    let key = match label {
        "全部体裁" => "newDrama.upcomingPage.genreAll",
        "真人剧" => "newDrama.calendarKind.liveAction",
        "漫剧" => "newDrama.calendarKind.comic",
        "全部受众" => "newDrama.upcomingPage.audienceAll",
        "男频" => "newDrama.upcomingPage.audienceMale",
        "女频" => "newDrama.upcomingPage.audienceFemale",
        "最高期待" => "newDrama.upcomingPage.mostAnticipated",
        "最快上线" => "newDrama.upcomingPage.soonest",
        "最新可约" => "newDrama.upcomingPage.latest",
        _ => return label.to_owned().into(),
    };
    tr(key)
}
