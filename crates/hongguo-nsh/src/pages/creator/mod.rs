mod fixtures;
mod model;
mod presentation;
#[cfg(test)]
mod tests;

use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_api::danmaku::{CreatorProfile, CreatorWorksPage};
use model::Query;
use nsh_core::prelude::*;
use std::collections::HashSet;

type Profile = Resource<CreatorProfile, bridge::Failure>;
type Works = Resource<CreatorWorksPage, bridge::Failure>;

pub(super) fn page(session: SessionHandle, nav: Navigation, user_id: String) -> Screen {
    if user_id.is_empty() {
        return empty(nav);
    }
    let uid = stored(user_id);
    let active = computed(move || {
        nav.routes()
            .with(|routes| matches!(routes.last(), Some(Route::Creator(id)) if *id==uid.get()))
    });
    let profile = resource(
        move || active.get().then(|| (uid.get(), session.identity())),
        move |(id, _), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                hongguo_api::danmaku::fetch_creator_profile(&id, &state.api_env()).await
            })
        },
    );
    let query = signal(Query {
        identity: session.identity(),
        ..Default::default()
    });
    effect(move || {
        let identity = session.identity();
        if query.get_untracked().identity != identity {
            query.set(Query {
                identity,
                ..Default::default()
            });
        }
    });
    let works: Works = resource(
        move || active.get().then(|| query.get()),
        move |query, _| {
            let state = session.with(|session| session.state.clone());
            let uid = uid.get();
            bridge::request(async move {
                let page = hongguo_api::danmaku::fetch_creator_works(
                    &uid,
                    &query.cursor,
                    &state.api_env(),
                )
                .await?;
                Ok(query.finish(page))
            })
        },
    );
    let prefetched = stored(HashSet::<String>::new());
    let prefetch = move |id: String| {
        if id.is_empty() || !prefetched.with(|ids| ids.insert(id.clone())) {
            return;
        }
        let state = session.with(|session| session.state.clone());
        spawn(async move {
            let result = bridge::request(async move {
                let series = hongguo_service::series_service::resolver::resolve_series(
                    &id,
                    &state.api_env(),
                )
                .await?;
                hongguo_service::series_service::registry::upsert_and_persist(&state, series)
            })
            .await;
            if let Err(error) = result {
                log::debug!("[NSH creator] optional work prefetch failed: {error}");
            }
        });
    };
    screen(nav, profile).child(presentation::content(
        nav,
        profile,
        works,
        move |id| open_work(nav, works, &id),
        prefetch,
        move || next_page(query, works, session.identity()),
    ))
}

fn next_page(query: Signal<Query>, works: Works, identity: u64) {
    if !works.loading()
        && works.error().is_none()
        && let Some(page) = works.get()
        && let Some(next) = query.with(|query| query.next(&page, identity))
    {
        query.set(next);
    }
}

pub(super) fn preview(nav: Navigation, user_id: String) -> Screen {
    if user_id.is_empty() {
        return empty(nav);
    }
    let profile: Profile = resource(
        || Some(()),
        |_, _| std::future::ready(Ok(fixtures::profile())),
    );
    let works: Works = resource(
        || Some(()),
        |_, _| std::future::ready(Ok(fixtures::works())),
    );
    screen(nav, profile).child(presentation::content(
        nav,
        profile,
        works,
        move |id| open_work(nav, works, &id),
        |_| {},
        || {},
    ))
}

pub(super) fn preview_creators() -> Vec<hongguo_api::detail::SeriesCreator> {
    fixtures::creators()
}

fn open_work(nav: Navigation, works: Works, id: &str) {
    if let Some(seed) = works.with(|page| {
        page.and_then(|page| page.works.iter().find(|item| item.series_id == id))
            .map(crate::navigation::DetailPrefill::from_work)
    }) {
        nav.detail(seed);
    }
}

fn screen(nav: Navigation, profile: Profile) -> Screen {
    ui::page(nav, move || {
        profile.with(|profile| {
            profile
                .filter(|profile| !profile.user_name.is_empty())
                .map(|profile| SharedString::from(profile.user_name.clone()))
                .unwrap_or_else(|| tr("creator.title"))
        })
    })
}

fn empty(nav: Navigation) -> Screen {
    ui::page(nav, move || tr("creator.title")).child(
        view().flex_1().items_center().justify_center().p_6().child(
            text(move || tr("detail.noSeries"))
                .text_sm()
                .text_color(move || nsh_base::theme().muted_foreground),
        ),
    )
}
