use crate::{bootstrap::Session, bridge};
use hongguo_common::{AppError, model::Series};
use std::{future::Future, pin::Pin, rc::Rc, sync::Arc};

pub(super) type ResolveFuture = Pin<Box<dyn Future<Output = Result<Series, bridge::Failure>>>>;
pub(super) type Resolve = Rc<dyn Fn(String) -> ResolveFuture>;

pub(super) fn resolver(session: Arc<Session>) -> Resolve {
    Rc::new(move |id| {
        let session = session.clone();
        Box::pin(bridge::request(async move {
            let series = super::super::start::resolve_archive(session.clone(), id).await?;
            let stored = series.clone();
            hongguo_service::runtime::spawn_blocking(move || {
                hongguo_service::series_service::registry::upsert_and_persist(
                    &session.state,
                    stored,
                )
            })
            .await
            .map_err(|error| AppError::Internal(error.to_string()))??;
            Ok(series)
        }))
    })
}
