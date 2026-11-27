pub(super) mod actions;
mod date;
mod page;
mod view;

use super::SessionHandle;
use crate::{
    bridge,
    navigation::{DetailPrefill, Navigation, Route},
};
use hongguo_service::contracts::calendar::{
    CalendarCursor, CalendarFilteredPage, CalendarPage, CalendarPanel, SelectorId,
};
use nsh_core::prelude::*;
use std::future::Future;

#[derive(Clone, PartialEq)]
struct FilteredPage {
    page: CalendarPage,
    cursor: CalendarCursor,
    filter: Option<SelectorId>,
    date: String,
    identity: u64,
}

#[derive(Clone, Default, PartialEq)]
struct Query {
    date: String,
    previous: Option<FilteredPage>,
    identity: u64,
    filter: Option<SelectorId>,
}

pub(super) fn content(
    session: SessionHandle,
    nav: Navigation,
    active: Memo<bool>,
    reservations: actions::Reservations,
) -> View {
    let panel = resource(
        move || active.get().then(|| session.identity()),
        move |identity, _| {
            let state = session.with(|session| session.state.clone());
            async move {
                bridge::request(async move {
                    hongguo_service::catalog_service::calendar_panel(&state).await
                })
                .await
                .map(|panel| (identity, panel))
            }
        },
    );
    let (selected, filter, query, feed) = data(
        active,
        move || session.identity(),
        panel,
        move |query, panel| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move { page::load(query, panel.as_ref(), &state).await })
        },
    );
    let playing = signal(false);
    view::presentation(
        view::State {
            feed,
            selected,
            filter,
            panel,
            identity: computed(move || session.identity()),
            fetching_more: computed(move || {
                feed.loading() && query.with(|query| query.previous.is_some())
            }),
            reservations,
            today: date::today(),
        },
        view::Events {
            open: Box::new(move |item| nav.detail(DetailPrefill::from_calendar(&item))),
            play: Box::new(move |item| {
                if playing.get_untracked() {
                    return;
                }
                playing.set(true);
                let identity = session.identity();
                let state = session.with(|session| session.clone());
                spawn(async move {
                    let result = bridge::request(crate::features::player::resolve_start(
                        state,
                        item.series_id,
                    ))
                    .await;
                    playing.set(false);
                    if session.identity() != identity {
                        return;
                    }
                    match result {
                        Ok((id, episode)) if active.get_untracked() => {
                            nav.push(Route::Player(id, episode))
                        }
                        Ok(_) => {}
                        Err(error) => reservations.message.set(error.text()),
                    }
                });
            }),
            reserve: Box::new(move |item| reservations.reserve(session, nav, item)),
            more: Box::new(move || {
                if !active.get_untracked() || feed.loading() || feed.error().is_some() {
                    return;
                }
                let date = selected.get_untracked();
                let filter = filter.get_untracked();
                let identity = session.identity();
                if let Some(result) = feed.get().filter(|result| {
                    result.page.has_more
                        && result.page.next_offset > 0
                        && result.date == date
                        && result.filter == filter
                        && result.identity == identity
                }) {
                    query.set(Query {
                        date,
                        filter,
                        previous: Some(result),
                        identity,
                    });
                }
            }),
        },
    )
}

fn data<Fut: Future<Output = Result<FilteredPage, bridge::Failure>> + 'static>(
    active: Memo<bool>,
    identity: impl Fn() -> u64 + Copy + 'static,
    panel: view::Panel,
    fetch: impl Fn(Query, Option<CalendarPanel>) -> Fut + 'static,
) -> (
    Signal<String>,
    Signal<Option<SelectorId>>,
    Signal<Query>,
    view::Feed,
) {
    crate::view_memory::cached("new-calendar", move || {
        let selected = crate::view_memory::scope("new").signal("date", String::new());
        let filter = signal(None);
        let query = signal(Query {
            identity: identity(),
            ..Default::default()
        });
        crate::view_memory::track_identity(identity, query, |query, identity| {
            query.identity = identity;
            query.previous = None;
            query.filter = None;
        });
        effect(move || {
            let _ = identity();
            filter.set(None);
        });
        effect(move || {
            let date = selected.get();
            let filter = filter.get();
            if query.with(|query| query.date != date || query.filter != filter) {
                query.set(Query {
                    date,
                    filter,
                    identity: identity(),
                    ..Default::default()
                });
            }
        });
        let feed = resource(
            move || {
                if !active.get() {
                    return None;
                }
                let identity = identity();
                let panel_value = panel.get().filter(|(owner, _)| *owner == identity);
                if panel_value.is_none() && panel.error().is_none() {
                    return None;
                }
                Some((query.get(), identity, panel_value.map(|(_, panel)| panel)))
            },
            move |(mut query, identity, panel), _| {
                if query.identity != identity {
                    query.identity = identity;
                    query.previous = None;
                    query.filter = None;
                }
                fetch(query, panel)
            },
        );
        (selected, filter, query, feed)
    })
}
