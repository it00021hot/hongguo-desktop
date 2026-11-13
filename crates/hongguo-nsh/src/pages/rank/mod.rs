mod filters;
mod model;
mod presentation;
mod preview;
#[cfg(test)]
mod responsive_tests;

use super::{SessionHandle, series_feed};
use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_api::rank::RankReq;
use model::Query;
pub(super) use model::ScrollScope;
use nsh_core::prelude::*;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    ui::page(nav, move || tr("nav.rank.title")).child(content(session, nav))
}

fn content(session: SessionHandle, nav: Navigation) -> View {
    let (query, source, paging) = crate::view_memory::cached("rank", move || {
        let query = crate::view_memory::scope("rank").projected(
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
        let source = resource(
            move || {
                nav.routes()
                    .with(|routes| routes.last() == Some(&Route::Rank))
                    .then(|| (query.get(), session.identity()))
            },
            move |(mut query, identity), _| {
                if query.identity != identity {
                    query.reset_paging();
                }
                let state = session.with(|session| session.state.clone());
                bridge::request(async move {
                    let mut page = hongguo_api::rank::fetch_rank_ex(
                        RankReq {
                            selected: &query.tab,
                            sub: &query.sub,
                            panel: Some(&query.panel),
                            background: Some(&query.background),
                            offset: query.offset,
                            session_id: &query.session,
                            rank_version: &query.version,
                        },
                        &state.api_env(),
                    )
                    .await?;
                    page.items = series_feed::append(query.previous, page.items);
                    Ok(page)
                })
            },
        );
        let paging = series_feed::Paging {
            loading: computed(move || source.loading() && query.with(|query| query.offset > 0)),
            request: stored(Box::new(move || {
                if source.loading() {
                    return;
                }
                if let Some(page) = source.get()
                    && page.has_more
                    && page.next_offset > query.with_untracked(|query| query.offset)
                {
                    query.update(|query| {
                        query.identity = session.identity();
                        query.offset = page.next_offset;
                        query.session = page.session_id;
                        query.version = page.rank_version;
                        query.previous = page.items;
                    });
                }
            }) as Box<dyn Fn()>),
        };
        (query, source, paging)
    });
    let rows = view().flex_1().min_h_0().child(each(
        move || {
            let query = query.get();
            vec![(query.tab, query.sub, query.panel, query.background)]
        },
        |key: &(String, String, String, String)| key.clone(),
        move |_, _| {
            series_feed::rank_list(
                source,
                session,
                nav,
                paging,
                computed(move || query.with(Query::scroll_scope)),
            )
        },
    ));
    presentation::content(nav, query, source, rows)
}

pub(super) fn preview(nav: Navigation) -> Screen {
    preview::build(nav)
}
