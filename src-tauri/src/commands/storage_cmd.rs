//! 磁盘占用与文件清理。
//!
//! 「移除列表」与「删除文件」是两个独立操作——这里分开暴露，
//! 避免误删时无法恢复剧集档案。

use tauri::State;

use crate::app_state::AppState;
use crate::error::AppResult;
use crate::service::storage_service::{cleanup, usage, usage::StorageUsage};

/// 本地占用统计。
#[tauri::command]
pub fn get_storage_usage(state: State<'_, AppState>) -> StorageUsage {
    usage::usage(&state)
}

/// 删除某部剧的全部本地文件（含合并产物与残留临时文件）。
///
/// 剧集档案保留，之后仍可在线播放或重新下载。
#[tauri::command]
pub fn delete_series_files(state: State<'_, AppState>, series_id: String) -> AppResult<usize> {
    let n = cleanup::delete_series(&state, &series_id)?;
    after_cleanup(&state)?;
    Ok(n)
}

/// 删除单集文件。
#[tauri::command]
pub fn delete_episode_file(
    state: State<'_, AppState>,
    series_id: String,
    vid_index: u32,
) -> AppResult<bool> {
    let ok = cleanup::delete_episode(&state, &series_id, vid_index)?;
    after_cleanup(&state)?;
    Ok(ok)
}

/// 删除全部已下载。
#[tauri::command]
pub fn delete_all_downloaded(state: State<'_, AppState>) -> AppResult<usize> {
    let n = cleanup::delete_all(&state)?;
    after_cleanup(&state)?;
    Ok(n)
}

/// 清理后同步任务状态并落盘。
fn after_cleanup(state: &State<'_, AppState>) -> AppResult<()> {
    crate::store::persist_tasks(state, "Storage")
}
