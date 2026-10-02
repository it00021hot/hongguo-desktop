//! 剧集档案持久化。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::Series;
use crate::error::AppResult;

/// 登记剧集并落盘。
pub fn upsert_and_persist(state: &State<'_, AppState>, series: Series) -> AppResult<()> {
    {
        let mut data = state.store.write();
        data.upsert_series(series);
    }
    let data = state.store.read();
    data.save(&crate::store::paths::data_file())
        .map_err(|e| crate::error::AppError::StoreCorrupt(e.to_string()))
}
