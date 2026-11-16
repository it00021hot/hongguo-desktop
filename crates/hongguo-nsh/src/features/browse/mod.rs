mod filters;
mod model;
mod preferences;
pub(crate) mod presentation;
mod search;

use super::{SessionHandle, series_feed};
use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
};
use hongguo_service::contracts::rank::{RankItem, RankPage};
use model::{LoadedPage, Query};
use nsh_core::prelude::*;

type Feed = Resource<LoadedPage, bridge::Failure>;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    crate::shell::page(nav, move || tr("nav.browse.title")).child(content(session, nav))
}

fn content(session: SessionHandle, nav: Navigation) -> View {
    let memory = crate::view_memory::scope("browse");
    let draft = memory.signal("draft", String::new());
    let (query, active, panel, feed) = crate::view_memory::cached("browse", move || {
        let query = memory.projected(
            "query",
            Query {
                identity: session.identity(),
                ..Default::default()
            },
            Query::view_snapshot,
        );
        crate::view_memory::track_identity(
            move || session.identity(),
            query,
            |query, identity| {
                query.identity = identity;
                query.reset_paging();
            },
        );
        let active = computed(move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&Route::Browse))
        });
        let panel = resource(
            move || active.get().then(|| session.identity()),
            move |_, _| {
                let state = session.with(|session| session.state.clone());
                bridge::request(async move {
                    hongguo_service::catalog_service::browse_panel(&state).await
                })
            },
        );
        let feed = resource(
            move || active.get().then(|| (query.get(), session.identity())),
            move |(mut query, identity), _| {
                if query.identity != identity {
                    query.reset_paging();
                }
                let state = session.with(|session| session.state.clone());
                bridge::request(async move {
                    let scope = query.scope();
                    let mut page = if query.term.is_empty() {
                        series_feed::from_feed(
                            hongguo_service::catalog_service::browse(
                                &query.filters,
                                query.offset,
                                &query.session,
                                &state,
                            )
                            .await?,
                        )
                    } else {
                        let page = hongguo_service::catalog_service::search(
                            &query.term,
                            query.offset,
                            &query.session,
                            &state,
                        )
                        .await?;
                        RankPage {
                            items: page
                                .items
                                .into_iter()
                                .map(|item| RankItem {
                                    series_id: item.series_id,
                                    title: item.title,
                                    cover: item.cover,
                                    vid: item.vid,
                                    episode_cnt: item.episode_cnt,
                                    sub_title: item.sub_title,
                                    score: item.score,
                                    description: item.description,
                                    tags: item.tags,
                                    play_cnt: item.play_cnt,
                                    rec_text: item.heat_text,
                                    ..Default::default()
                                })
                                .collect(),
                            has_more: page.has_more,
                            next_offset: page.next_offset,
                            session_id: page.search_id,
                            ..Default::default()
                        }
                    };
                    page.items = series_feed::append(query.previous, page.items);
                    Ok(LoadedPage { scope, page })
                })
            },
        );
        (query, active, panel, feed)
    });
    let downloaded = computed(move || {
        let _revision = session.downloads();
        session.with(|session| {
            let mut counts = std::collections::HashMap::<String, usize>::new();
            for task in session.state.queue.read().all() {
                if task.status == hongguo_common::model::TaskStatus::Completed {
                    *counts.entry(task.series_id.clone()).or_default() += 1;
                }
            }
            counts
        })
    });
    let search = search::Search::new(
        draft,
        move || {
            (active.get() && query.with(|query| query.term.is_empty())).then(|| session.identity())
        },
        move |term| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                hongguo_service::catalog_service::suggestions(&term, &state).await
            })
        },
    );
    presentation::content(nav, search, query, panel, feed, downloaded, move || {
        next_page(query, feed, session.identity());
    })
}

fn next_page(query: Signal<Query>, feed: Feed, identity: u64) {
    if !feed.loading()
        && feed.error().is_none()
        && let Some(LoadedPage { scope, page }) = feed.get()
        && page.has_more
        && scope == query.with(Query::scope)
        && page.next_offset > 0
        && query.with(|query| query.identity != identity || page.next_offset > query.offset)
    {
        query.update(|query| {
            query.identity = identity;
            query.offset = page.next_offset;
            query.session = page.session_id;
            query.previous = page.items;
        });
    }
}
