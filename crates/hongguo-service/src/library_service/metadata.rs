use std::collections::{HashMap, HashSet};

use crate::AppState;
use futures_util::{StreamExt, stream};
use hongguo_api::detail;
use hongguo_common::{AppError, AppResult};

use super::model::Entry;

pub const METADATA_WINDOW_BATCH: usize = 6;
const SINGLE_DETAIL_CONCURRENCY: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollectionMetadata {
    Ready {
        title: String,
        cover: String,
        episode_count: i64,
    },
    Unresolved,
    Failed,
}

impl CollectionMetadata {
    pub fn apply(&self, entry: &mut Entry) {
        if let Self::Ready {
            title,
            cover,
            episode_count,
        } = self
        {
            if !title.trim().is_empty() {
                entry.title.clone_from(title);
            }
            if !cover.is_empty() {
                entry.cover.clone_from(cover);
            }
            entry.episode_count = *episode_count;
            entry.can_play = !entry.title.trim().is_empty() && *episode_count > 0;
        }
    }
}

pub async fn collection_metadata(
    state: AppState,
    ids: Vec<String>,
) -> AppResult<HashMap<String, CollectionMetadata>> {
    let settings = state.settings();
    if settings.account.is_none() {
        return Err(AppError::Auth("登录后可查看收藏资料".into()));
    }
    if ids.len() > METADATA_WINDOW_BATCH {
        return Err(AppError::InvalidArgs("单批收藏资料超过上限".into()));
    }
    let store = state.store.clone();
    let (mut metadata, missing) = crate::runtime::spawn_blocking(move || {
        let mut metadata = HashMap::new();
        let mut missing = Vec::new();
        let mut seen = HashSet::new();
        for id in ids.into_iter().filter(|id| seen.insert(id.clone())) {
            match store.series_by_id(&id)? {
                Some(series)
                    if !series.title.trim().is_empty()
                        && !series.cover.is_empty()
                        && series.episode_count > 0 =>
                {
                    metadata.insert(
                        id,
                        CollectionMetadata::Ready {
                            title: series.title,
                            cover: series.cover,
                            episode_count: series.episode_count.into(),
                        },
                    );
                }
                _ => missing.push(id),
            }
        }
        Ok::<_, AppError>((metadata, missing))
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    if missing.is_empty() {
        return Ok(metadata);
    }
    let env = state.api_env_from_settings(settings);
    let requested: HashSet<&str> = missing.iter().map(String::as_str).collect();
    // A partial result is not an availability verdict. A valid long series may
    // be omitted from the mixed preload response but resolve when queried alone.
    if missing.len() > 1 {
        match detail::fetch_episode_lists_batch(&missing, &env).await {
            Ok(series) => {
                for item in series {
                    if requested.contains(item.series_id.as_str()) && !item.title.trim().is_empty()
                    {
                        metadata.insert(item.series_id.clone(), ready(item));
                    }
                }
            }
            Err(AppError::Media(_)) => {}
            Err(error) => {
                log::warn!("[Library] collection batch metadata request failed: {error}");
                metadata.extend(
                    missing
                        .into_iter()
                        .map(|id| (id, CollectionMetadata::Failed)),
                );
                return Ok(metadata);
            }
        }
    }
    let missing = missing
        .into_iter()
        .filter(|id| !metadata.contains_key(id))
        .collect::<Vec<_>>();
    let singles = stream::iter(missing)
        .map(|id| {
            let env = &env;
            async move {
                let item = match detail::fetch_episode_list(&id, env).await {
                    Ok(item) if item.series_id == id && !item.title.trim().is_empty() => {
                        ready(item)
                    }
                    Ok(_) | Err(AppError::Media(_)) => CollectionMetadata::Unresolved,
                    Err(error) => {
                        log::warn!("[Library] collection single detail request failed: {error}");
                        CollectionMetadata::Failed
                    }
                };
                (id, item)
            }
        })
        .buffer_unordered(SINGLE_DETAIL_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    metadata.extend(singles);
    Ok(metadata)
}

fn ready(item: detail::EpisodeList) -> CollectionMetadata {
    CollectionMetadata::Ready {
        title: item.title,
        cover: item.cover,
        episode_count: item.episodes.len() as i64,
    }
}
