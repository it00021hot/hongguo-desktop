use hongguo_common::{AppError, AppResult, model::Series};
use hongguo_service::{runtime, series_service::registry};
use std::sync::Arc;

pub(crate) async fn resolve_start(
    session: Arc<crate::bootstrap::Session>,
    id: String,
) -> AppResult<(String, u32)> {
    let state = session.state.clone();
    let archive = resolve_archive(session.clone(), id);
    let (series, sources) = tokio::try_join!(archive, session.personal.read(&state))?;
    let snapshot = sources.clone();
    let working = state.clone();
    let (id, episode, at) = runtime::spawn_blocking(move || {
        let (preferred, at) = hongguo_service::play_service::resume::resolve_recent_entry(
            &working.store,
            &series.series_id,
            &snapshot.history,
        )?;
        let episode = start_episode(&series, Some(preferred))?;
        let id = series.series_id.clone();
        registry::upsert_and_persist(&working, series)?;
        Ok::<_, AppError>((id, episode, if episode == preferred { at } else { 0.0 }))
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    session
        .personal
        .remember(&state, &sources, id.clone(), episode, at)?;
    Ok((id, episode))
}

/// Keep an explicitly selected history episode without recording a feed resume point.
pub(crate) async fn resolve_history_start(
    session: Arc<crate::bootstrap::Session>,
    id: String,
    episode: u32,
) -> AppResult<String> {
    let series = resolve_archive(session.clone(), id).await?;
    let state = session.state.clone();
    runtime::spawn_blocking(move || {
        if !series.episodes.iter().any(|item| item.vid_index == episode) {
            return Err(AppError::NotFound(format!("历史记录中的第 {episode} 集")));
        }
        let id = series.series_id.clone();
        registry::upsert_and_persist(&state, series)?;
        Ok(id)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Resolve the archive without applying a history starting point. Explicit
/// season relay consumes the same archive and chooses its first real episode.
pub(super) async fn resolve_archive(
    session: Arc<crate::bootstrap::Session>,
    id: String,
) -> AppResult<Series> {
    let state = &session.state;
    let store = state.store.clone();
    let key = id.clone();
    let cached = runtime::spawn_blocking(move || store.series_by_id(&key))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
    match cached.filter(|series| !series.episodes.is_empty()) {
        Some(series) => Ok(series),
        None => hongguo_service::series_service::resolve(&id, state).await,
    }
}

pub(crate) fn start_episode(series: &Series, last: Option<u32>) -> AppResult<u32> {
    last.and_then(|last| {
        series
            .episodes
            .iter()
            .find(|episode| episode.vid_index == last)
    })
    .or_else(|| series.episodes.first())
    .map(|episode| episode.vid_index)
    .ok_or_else(|| AppError::NotFound("剧集暂无可播放分集".into()))
}
