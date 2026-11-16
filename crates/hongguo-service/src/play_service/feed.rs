use crate::{AppState, contracts::history::WatchHistoryItem, runtime, series_service};
use hongguo_common::{AppError, AppResult};

pub async fn prefetch(
    state: &AppState,
    series_ids: Vec<String>,
    history: Vec<WatchHistoryItem>,
) -> AppResult<()> {
    let budget = super::prefetch_budget(state).await;
    for id in series_ids.into_iter().take(budget.count as usize) {
        let working = state.clone();
        let series_id = id.clone();
        let archived = runtime::spawn_blocking(move || working.store.series_by_id(&series_id))
            .await
            .map_err(|error| AppError::Internal(error.to_string()))??;
        let series = match archived {
            Some(series) => series,
            None => series_service::resolve(&id, state).await?,
        };
        let working = state.clone();
        let recent = history.clone();
        let episode = runtime::spawn_blocking(move || {
            let (preferred, _) =
                super::resume::resolve_recent_entry(&working.store, &series.series_id, &recent)?;
            let episode = series
                .episodes
                .iter()
                .find(|episode| episode.vid_index == preferred)
                .or_else(|| {
                    series
                        .episodes
                        .iter()
                        .min_by_key(|episode| episode.vid_index)
                })
                .map(|episode| episode.vid_index);
            series_service::registry::upsert_and_persist(&working, series)?;
            Ok::<_, AppError>(episode)
        })
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
        let Some(episode) = episode else {
            continue;
        };
        if state
            .queue()
            .completed_path(&id, episode)
            .is_some_and(|path| std::path::Path::new(&path).is_file())
        {
            continue;
        }
        let scene = if episode == 1 {
            hongguo_api::params::PlayerModelScene::Preload
        } else {
            hongguo_api::params::PlayerModelScene::Player
        };
        super::local::prefetch_episode_models(state, &id, episode..=episode, scene).await;
        let Some(vid) = super::local::episode_vid(&state.store, &id, episode) else {
            continue;
        };
        super::online::prefetch_stream(state, &vid, &format!("{id}:{episode}"), budget.bytes)
            .await?;
    }
    Ok(())
}
