mod actions;
mod date;
mod preview;
mod view;

use super::SessionHandle;
use crate::{
    bridge,
    navigation::{DetailPrefill, Navigation, Route},
};
use hongguo_api::calendar::CalendarItem;
use hongguo_common::AppError;
use nsh_core::prelude::*;
use std::{collections::HashSet, future::Future};

#[derive(Clone, Default, PartialEq)]
pub(super) struct Query {
    pub(super) date: String,
    pub(super) offset: i64,
    pub(super) session: String,
    pub(super) previous: Vec<CalendarItem>,
    pub(super) identity: u64,
}

pub(super) fn content(session: SessionHandle, nav: Navigation, active: Memo<bool>) -> View {
    let (selected, query, feed) = data(
        active,
        move || session.identity(),
        move |query| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let date = (!query.date.is_empty()).then_some(query.date.as_str());
                let mut offset = query.offset;
                let mut page = hongguo_api::calendar::fetch_new_calendar(
                    date,
                    (offset > 0).then_some((offset, query.session.as_str())),
                    &state.api_env(),
                )
                .await?;
                let dates = page.dates.clone();
                let default_date = page.default_date.clone();
                while page.items.is_empty() && page.has_more {
                    if page.next_offset <= offset {
                        return Err(AppError::Media("Calendar cursor did not advance".into()));
                    }
                    offset = page.next_offset;
                    page = hongguo_api::calendar::fetch_new_calendar(
                        date,
                        Some((offset, &page.session_id)),
                        &state.api_env(),
                    )
                    .await?;
                }
                page.dates = dates;
                page.default_date = default_date;
                page.items = append(query.previous, page.items);
                Ok(page)
            })
        },
    );
    let reservations = actions::Reservations::observe(session);
    let playing = signal(false);
    view::presentation(
        view::State {
            feed,
            selected,
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
                    let result =
                        bridge::request(crate::player::resolve_start(state, item.series_id)).await;
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
                if let Some(page) = feed
                    .get()
                    .filter(|page| page.has_more && page.next_offset > 0)
                {
                    query.set(Query {
                        date: selected.get_untracked(),
                        offset: page.next_offset,
                        session: page.session_id,
                        previous: page.items,
                        identity: session.identity(),
                    });
                }
            }),
        },
    )
}

pub(super) fn preview(nav: Navigation) -> View {
    preview::content(nav)
}

pub(super) fn data<
    Fut: Future<Output = Result<hongguo_api::calendar::CalendarPage, bridge::Failure>> + 'static,
>(
    active: Memo<bool>,
    identity: impl Fn() -> u64 + Copy + 'static,
    fetch: impl Fn(Query) -> Fut + 'static,
) -> (Signal<String>, Signal<Query>, view::Feed) {
    crate::view_memory::cached("new-calendar", move || {
        let selected = crate::view_memory::scope("new").signal("date", String::new());
        let query = signal(Query {
            identity: identity(),
            ..Default::default()
        });
        crate::view_memory::track_identity(identity, query, |query, identity| {
            query.identity = identity;
            query.offset = 0;
            query.session.clear();
            query.previous.clear();
        });
        effect(move || {
            let date = selected.get();
            if query.with(|query| query.date != date) {
                query.set(Query {
                    date,
                    identity: identity(),
                    ..Default::default()
                });
            }
        });
        let feed = resource(
            move || active.get().then(|| (query.get(), identity())),
            move |(mut query, identity), _| {
                if query.identity != identity {
                    query.offset = 0;
                    query.session.clear();
                    query.previous.clear();
                }
                fetch(query)
            },
        );
        (selected, query, feed)
    })
}

fn append(mut previous: Vec<CalendarItem>, next: Vec<CalendarItem>) -> Vec<CalendarItem> {
    let mut seen = previous
        .iter()
        .map(|item| item.series_id.clone())
        .collect::<HashSet<_>>();
    previous.extend(
        next.into_iter()
            .filter(|item| seen.insert(item.series_id.clone())),
    );
    previous
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_pages_preserve_published_metadata_and_deduplicate_by_series() {
        let first = CalendarItem {
            series_id: "one".into(),
            publish_time: 42,
            ..Default::default()
        };
        let second = CalendarItem {
            series_id: "two".into(),
            is_online: true,
            ..Default::default()
        };
        let rows = append(vec![first.clone()], vec![first, second]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].publish_time, 42);
        assert!(rows[1].is_online);
        assert!(append(Vec::new(), Vec::new()).is_empty());
    }
}
