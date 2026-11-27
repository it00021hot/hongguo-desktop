//! 剧集档案持久化。

use crate::app_state::AppState;
use hongguo_common::AppResult;
use hongguo_common::model::Series;

/// 登记剧集并落库。
pub fn upsert_and_persist(state: &AppState, mut series: Series) -> AppResult<()> {
    // The lightweight episode archive can omit the feed's creator/bottom bar.
    if series.episodes.len() == 1
        && let Some(previous) = state
            .store
            .series_by_id(&series.series_id)?
            .and_then(|old| old.highlight)
    {
        match series.highlight.as_mut() {
            None => series.highlight = Some(previous),
            Some(highlight) => {
                if highlight.author.is_empty() {
                    highlight.author = previous.author;
                }
                if highlight.related.is_none() {
                    highlight.related = previous.related;
                }
                if highlight.episode_number.is_none() {
                    highlight.episode_number = previous.episode_number;
                }
            }
        }
    }
    state.store.upsert_series(&series)
}
