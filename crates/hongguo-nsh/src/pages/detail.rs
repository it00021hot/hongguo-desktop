use super::SessionHandle;
use crate::{bridge, i18n::tr, navigation::Navigation, ui};
use nsh_core::prelude::*;
mod creators;
mod data;
mod hero;
mod prefill;
mod presentation;
mod related;
mod reservation;
pub(crate) use prefill::DetailPrefill;
#[cfg(test)]
mod reservation_tests;
#[cfg(test)]
mod upcoming_tests;

pub(super) fn page(session: SessionHandle, nav: Navigation, id: String) -> Screen {
    ui::page(nav, move || tr("nav.detail.title")).child(content(session, nav, id, None))
}

pub(super) fn seeded_page(
    session: SessionHandle,
    nav: Navigation,
    prefill: DetailPrefill,
) -> Screen {
    ui::page(nav, move || tr("nav.detail.title")).child(content(
        session,
        nav,
        prefill.series_id.clone(),
        Some(prefill),
    ))
}

fn content(
    session: SessionHandle,
    nav: Navigation,
    id: String,
    prefill: Option<DetailPrefill>,
) -> impl IntoElement {
    let detail_id = stored(id.clone());
    let data::Requests {
        result,
        metadata,
        resume,
    } = data::load(session, nav, id.clone(), prefill.clone());
    let social = crate::player::detail_social::observe(session, nav, result, resume);
    let active = computed(move || {
        nav.routes().with(|routes| {
            routes
                .last()
                .is_some_and(|route| route.detail_id() == Some(detail_id.get().as_str()))
        })
    });
    let seeded = prefill.is_some();
    let reservation = reservation::observe(
        session,
        nav,
        detail_id,
        computed(move || seeded && active.get() && result.error().is_some()),
    );
    presentation::render(DetailState {
        nav,
        result,
        metadata,
        social,
        resume,
        related: related::load(session, id),
        session: Some(session),
        id: detail_id,
        prefill,
        reservation,
    })
}

type SeriesData = Resource<hongguo_common::model::Series, bridge::Failure>;
type MetaData = Resource<hongguo_api::detail::SeriesMeta, bridge::Failure>;
struct DetailState {
    nav: Navigation,
    result: SeriesData,
    metadata: MetaData,
    social: crate::player::detail_social::DetailSocial,
    resume: Resource<Option<u32>, bridge::Failure>,
    related: Resource<hongguo_api::detail::RelatedSeries, bridge::Failure>,
    session: Option<SessionHandle>,
    id: Stored<String>,
    prefill: Option<DetailPrefill>,
    reservation: reservation::Reservation,
}

pub(super) fn preview(
    nav: Navigation,
    series: hongguo_common::model::Series,
    meta: hongguo_api::detail::SeriesMeta,
) -> Screen {
    let id = stored(series.series_id.clone());
    let result: SeriesData = resource(
        move || Some(series.clone()),
        |series, _| std::future::ready(Ok(series)),
    );
    let metadata: MetaData = resource(
        move || Some(meta.clone()),
        |meta, _| std::future::ready(Ok(meta)),
    );
    ui::page(nav, move || tr("nav.detail.title")).child(presentation::render(DetailState {
        nav,
        result,
        metadata,
        social: crate::player::detail_social::preview(),
        resume: resource(|| Some(()), |_, _| std::future::ready(Ok(None))),
        related: related::preview(),
        session: None,
        id,
        prefill: None,
        reservation: reservation::preview(),
    }))
}

pub(super) fn preview_upcoming(nav: Navigation, prefill: DetailPrefill) -> Screen {
    let id = stored(prefill.series_id.clone());
    let result: SeriesData = resource(
        || Some(()),
        |_, _| {
            std::future::ready(Err(bridge::Failure::from(hongguo_common::AppError::Media(
                "offline upcoming has no episodes".into(),
            ))))
        },
    );
    let seed_meta = prefill.metadata();
    let metadata: MetaData = resource(
        || Some(()),
        move |_, _| std::future::ready(Ok(seed_meta.clone())),
    );
    ui::page(nav, move || tr("nav.detail.title")).child(presentation::render(DetailState {
        nav,
        result,
        metadata,
        social: crate::player::detail_social::preview(),
        resume: resource(|| Some(()), |_, _| std::future::ready(Ok(None))),
        related: related::preview(),
        session: None,
        id,
        prefill: Some(prefill),
        reservation: reservation::preview(),
    }))
}
