//! 云端观看历史 command。

use crate::app_state::AppState;
use crate::domain::api::history::{fetch_watch_history, WatchHistoryPage};
use crate::error::AppResult;
use tauri::State;

/// 拉一页云端观看历史（官方 App 侧栏「历史」同源）。
#[tauri::command]
pub async fn watch_history_list(
    state: State<'_, AppState>,
    offset: Option<i64>,
) -> AppResult<WatchHistoryPage> {
    let env = state.api_env();
    fetch_watch_history(offset.unwrap_or(0), &env).await
}
