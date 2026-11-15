use super::model::Entry;
use hongguo_common::AppResult;

pub(super) async fn start_playback(
    session: std::sync::Arc<crate::bootstrap::Session>,
    entry: Entry,
) -> AppResult<(String, u32)> {
    crate::features::player::resolve_start(session, entry.series_id).await
}
