use super::model::Query;
pub(super) use crate::features::player::resolve_feed_start as target;
use hongguo_common::AppResult;
use hongguo_service::contracts::discover::FeedPage;
use std::sync::Arc;

pub(super) async fn feed(
    session: Arc<crate::bootstrap::Session>,
    query: Query,
) -> AppResult<FeedPage> {
    let state = &session.state;
    hongguo_service::catalog_service::recommendations(
        &query.channel,
        &query.session,
        query.offset,
        &query.seen,
        state,
    )
    .await
}
