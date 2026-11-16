use super::SessionHandle;
use crate::{bridge, i18n::tr, navigation::Navigation};
use nsh_core::prelude::*;
mod creators;
mod data;
mod hero;
mod prefill;
pub(crate) mod presentation;
mod related;
mod reservation;
pub(crate) use prefill::DetailPrefill;

pub(super) fn page(session: SessionHandle, nav: Navigation, id: String) -> Screen {
    crate::shell::page(nav, move || tr("nav.detail.title")).child(content(session, nav, id, None))
}

pub(super) fn seeded_page(
    session: SessionHandle,
    nav: Navigation,
    prefill: DetailPrefill,
) -> Screen {
    crate::shell::page(nav, move || tr("nav.detail.title")).child(content(
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
    let social = crate::features::player::detail_social::observe(session, nav, result, resume);
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
        session,
        id: detail_id,
        prefill,
        reservation,
    })
}

type SeriesData = Resource<hongguo_common::model::Series, bridge::Failure>;
type MetaData = Resource<hongguo_service::contracts::detail::SeriesMeta, bridge::Failure>;
struct DetailState {
    nav: Navigation,
    result: SeriesData,
    metadata: MetaData,
    social: crate::features::player::detail_social::DetailSocial,
    resume: Resource<Option<u32>, bridge::Failure>,
    related: Resource<hongguo_service::contracts::detail::RelatedSeries, bridge::Failure>,
    session: SessionHandle,
    id: Stored<String>,
    prefill: Option<DetailPrefill>,
    reservation: reservation::Reservation,
}
