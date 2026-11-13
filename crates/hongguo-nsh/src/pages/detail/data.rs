use super::{DetailPrefill, MetaData, SeriesData};
use crate::{bridge, navigation::Navigation, pages::SessionHandle};
use nsh_core::prelude::*;

pub(super) struct Requests {
    pub result: SeriesData,
    pub metadata: MetaData,
    pub resume: Resource<Option<u32>, bridge::Failure>,
}

pub(super) fn load(
    session: SessionHandle,
    nav: Navigation,
    id: String,
    seed: Option<DetailPrefill>,
) -> Requests {
    let position_id = id.clone();
    let resume = resource(
        move || {
            nav.routes()
                .with(|routes| {
                    routes
                        .last()
                        .is_some_and(|route| route.detail_id() == Some(position_id.as_str()))
                })
                .then(|| (position_id.clone(), session.identity(), session.library()))
        },
        move |(id, _, _), _| {
            let state = session.with(|session| session.clone());
            bridge::request(async move {
                let sources = state.personal.read(&state.state).await?;
                let Some(item) = sources.history.iter().find(|item| item.series_id == id) else {
                    return Ok(None);
                };
                let episode = u32::try_from(item.vid_index).map_err(|_| {
                    hongguo_common::AppError::InvalidArgs("历史集号超出有效范围".into())
                })?;
                state.personal.remember(
                    &state.state,
                    &sources,
                    id,
                    episode,
                    item.position_ms as f64 / 1000.0,
                )?;
                Ok(Some(episode))
            })
        },
    );
    let meta_id = id.clone();
    let metadata = resource(
        move || Some((meta_id.clone(), session.identity())),
        move |(id, _), _| {
            let state = session.with(|session| session.state.clone());
            let fallback = seed.as_ref().map(DetailPrefill::metadata);
            bridge::request(async move {
                match hongguo_api::detail::fetch_series_meta(&id, &state.api_env()).await {
                    Ok(meta) => Ok(meta),
                    Err(_) if fallback.is_some() => Ok(fallback.expect("checked source snapshot")),
                    Err(error) => Err(error),
                }
            })
        },
    );
    let result = resource(
        move || Some((id.clone(), session.identity())),
        move |(id, _), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let series = hongguo_service::series_service::resolver::resolve_series(
                    &id,
                    &state.api_env(),
                )
                .await?;
                // Only a real resolved archive belongs in the store. Source snapshots stay in this view.
                hongguo_service::series_service::registry::upsert_and_persist(
                    &state,
                    series.clone(),
                )?;
                Ok(series)
            })
        },
    );
    Requests {
        result,
        metadata,
        resume,
    }
}
