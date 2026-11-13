mod filters;
mod model;
mod preferences;
mod presentation;
mod search;
mod suggestion_fixtures;

#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod lifetime_tests;
#[cfg(test)]
mod tests;

use super::{SessionHandle, series_feed};
use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_api::rank::{RankItem, RankPage};
use model::{LoadedPage, Query};
use nsh_core::prelude::*;

type Feed = Resource<LoadedPage, bridge::Failure>;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    ui::page(nav, move || tr("nav.browse.title")).child(content(session, nav))
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
                    hongguo_api::discover::fetch_browse_panel(&state.api_env()).await
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
                            hongguo_api::discover::fetch_browse(
                                &query.filters,
                                query.offset,
                                &query.session,
                                &state.api_env(),
                            )
                            .await?,
                        )
                    } else {
                        let page = hongguo_api::search::search_series(
                            &query.term,
                            query.offset,
                            &query.session,
                            &state.api_env(),
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
                hongguo_api::search::search_suggest(&term, &state.api_env()).await
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

pub(super) fn preview(nav: Navigation) -> Screen {
    let catalog = super::fixtures::catalog();
    let rows = stored(catalog.rows);
    let page = stored(series_feed::from_feed(hongguo_api::discover::FeedPage {
        items: catalog.items,
        ..Default::default()
    }));
    let panel = resource(
        move || Some(rows.get()),
        |rows, _| std::future::ready(Ok(rows)),
    );
    let downloaded = computed(move || {
        page.with(|page| {
            page.items
                .first()
                .map(|item| (item.series_id.clone(), 1_usize))
                .into_iter()
                .collect()
        })
    });
    let memory = crate::view_memory::scope("browse");
    let query = memory.projected("query", Query::default(), Query::view_snapshot);
    let feed: Feed = resource(
        move || Some(query.with(Query::scope)),
        move |scope, _| {
            std::future::ready(Ok(LoadedPage {
                scope,
                page: page.get(),
            }))
        },
    );
    let search = search::Search::new(
        memory.signal("draft", String::new()),
        move || {
            (query.with(|query| query.term.is_empty())
                && nav
                    .routes()
                    .with(|routes| routes.last() == Some(&Route::Browse)))
            .then_some(0)
        },
        |term| std::future::ready(Ok(suggestion_fixtures::suggestions(&term))),
    );
    ui::page(nav, move || tr("nav.browse.title")).child(presentation::content(
        nav,
        search,
        query,
        panel,
        feed,
        downloaded,
        || {},
    ))
}
