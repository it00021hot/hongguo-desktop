//! 云端观看历史 command。

use crate::app_state::AppState;
use crate::domain::api::history::{fetch_watch_history, report_watch_progress, WatchHistoryPage};
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

/// 观看进度云上报（read_history/update + read_progress/upload，hgplayer
/// 同款双接口）。匿名时静默跳过——上报是 best-effort，不阻塞播放、
/// 失败只记日志（官方客户端对它的失败也是静默的）。
#[tauri::command]
pub async fn cloud_report_progress(
    state: State<'_, AppState>,
    series_id: String,
    vid: String,
    vid_index: i64,
    position_ms: i64,
) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Ok(());
    }
    let env = state.api_env();
    match report_watch_progress(&series_id, &vid, vid_index, position_ms.max(0), &env).await {
        Ok(()) => Ok(()),
        Err(e) => {
            log::warn!("[History] 云端进度上报失败（忽略）: {e}");
            Ok(())
        }
    }
}
