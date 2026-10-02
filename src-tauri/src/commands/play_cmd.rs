//! 播放。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::{PlayRequest, PlayResponse};
use crate::error::AppResult;

#[tauri::command]
pub async fn play_series(
    state: State<'_, AppState>,
    request: PlayRequest,
) -> AppResult<PlayResponse> {
    crate::service::play_service::local::resolve_play(&state, &request).await
}

#[tauri::command]
pub fn save_playback_position(
    state: State<'_, AppState>,
    series_id: String,
    vid_index: u32,
    current_time: f64,
    duration: f64,
) -> AppResult<()> {
    crate::service::play_service::position::save(
        &state,
        &series_id,
        vid_index,
        current_time,
        duration,
    )
}

/// 播放历史（每部剧最近看到的一集，按时间倒序）。
#[tauri::command]
pub fn get_playback_history(
    state: State<'_, AppState>,
) -> Vec<crate::domain::model::PlaybackHistoryItem> {
    crate::service::play_service::position::history(&state)
}
