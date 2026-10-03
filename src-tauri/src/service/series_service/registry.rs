//! 剧集档案持久化。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::Series;
use crate::error::AppResult;

/// 登记剧集并落库。
pub fn upsert_and_persist(state: &State<'_, AppState>, series: Series) -> AppResult<()> {
    state.store.upsert_series(&series)
}
