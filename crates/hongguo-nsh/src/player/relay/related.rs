use crate::{bridge, events::SessionHandle};
use hongguo_api::detail::RelatedSeries;
use hongguo_common::{AppError, model::Series};
use nsh_core::prelude::*;

#[derive(Clone, PartialEq)]
pub(crate) struct Snapshot {
    pub id: String,
    pub identity: u64,
    pub current: Option<Series>,
    pub related: RelatedSeries,
}

pub(crate) type Related = Resource<Snapshot, bridge::Failure>;

pub(super) fn observe(
    session: SessionHandle,
    selection: Signal<(String, u32)>,
    active: Memo<bool>,
) -> Related {
    resource(
        move || {
            active
                .get()
                .then(|| (selection.get().0, session.identity()))
        },
        move |(id, identity), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let related =
                    hongguo_api::detail::fetch_related_series(&id, &state.api_env()).await?;
                let store = state.store.clone();
                let key = id.clone();
                let current =
                    hongguo_service::runtime::spawn_blocking(move || store.series_by_id(&key))
                        .await
                        .map_err(|error| AppError::Internal(error.to_string()))??;
                Ok(Snapshot {
                    id,
                    identity,
                    current,
                    related,
                })
            })
        },
    )
}
