use std::collections::{BTreeMap, HashSet};

use hongguo_api::{detail, history::WatchHistoryItem, params::PlayerModelScene, play_url};
use hongguo_common::{AppResult, model::Series};

use crate::app_state::AppState;
use crate::series_service::{registry, resolver};

const ARCHIVE_BATCH_SIZE: usize = 6;

pub async fn prefetch_feed(
    state: &AppState,
    series_ids: Vec<String>,
    history: &[WatchHistoryItem],
) -> AppResult<Vec<String>> {
    let budget = super::prefetch_budget(state).await;
    let mut seen = HashSet::new();
    let ids: Vec<_> = series_ids
        .into_iter()
        .filter(|id| {
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && seen.insert(id.clone())
        })
        .take(budget.count as usize)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let env = state.api_env();
    let mut archives = BTreeMap::new();
    let mut missing = Vec::new();
    for id in &ids {
        if let Some(series) = state
            .store
            .series_by_id(id)?
            .filter(|s| !s.episodes.is_empty())
        {
            archives.insert(id.clone(), series);
        } else {
            missing.push(id.clone());
        }
    }
    for batch in missing.chunks(ARCHIVE_BATCH_SIZE) {
        let lists = match detail::fetch_episode_lists_batch(batch, &env).await {
            Ok(lists) => lists,
            Err(error) => {
                log::debug!("[Online] 首页批量档案预取失败: {error}");
                continue;
            }
        };
        for list in lists {
            let series = resolver::series_from_list(list);
            registry::upsert_and_persist(state, series.clone())?;
            archives.insert(series.series_id.clone(), series);
        }
    }

    let mut models: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut media = Vec::new();
    for id in &ids {
        let Some(series) = archives.get(id) else {
            continue;
        };
        let (vid_index, _) = super::resume::resolve_entry(&state.store, id, history)?;
        if let Some(episode) = entry_episode(series, vid_index) {
            if series.content_type != 0 {
                models
                    .entry(series.content_type.to_string())
                    .or_default()
                    .push(episode.vid.clone());
            }
            media.push((id, episode.vid_index));
        }
    }
    if !models.is_empty() {
        crate::cold_start::wait().await;
        if let Err(error) = play_url::fetch_play_scene(
            &models,
            PlayerModelScene::Preload,
            crate::device::stream_cap(),
            &env,
        )
        .await
        {
            log::debug!("[Online] 首页批量模型预取失败: {error}");
        }
    }
    let mut scheduled = Vec::with_capacity(media.len());
    for (id, index) in media {
        super::online::schedule_next_episode_prefetch(state.clone(), id, index, budget.bytes);
        scheduled.push(id.clone());
    }
    Ok(scheduled)
}

fn entry_episode(series: &Series, index: u32) -> Option<&hongguo_common::model::Episode> {
    series
        .episodes
        .iter()
        .find(|e| e.vid_index == index && !e.vid.is_empty())
        .or_else(|| {
            series
                .episodes
                .iter()
                .find(|e| e.vid_index == 1 && !e.vid.is_empty())
        })
}
