//! 下载任务的删除与补登记。
//!
//! 删除与补登记都是「动了文件」的操作，放一起便于审查副作用。

use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};

use super::persist;
use crate::service::download_service::events::names;

/// 删除任务；`delete_files` 为真时同时删除本地文件。
#[tauri::command]
pub fn delete_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    task_ids: Vec<String>,
    delete_files: Option<bool>,
) -> AppResult<usize> {
    let with_files = delete_files.unwrap_or(false);

    if with_files {
        // 逐个删文件（删任务与删文件是两个独立操作，这里按用户勾选联动）
        for id in &task_ids {
            if let Some(task) = state.queue().get(id) {
                if task.is_done() {
                    let path = std::path::Path::new(&task.file_path);
                    if path.exists() {
                        let _ = std::fs::remove_file(path);
                    }
                    crate::service::download_service::worker::cleanup_temp(path);
                }
            }
        }
    }

    let n = state.queue().remove(&task_ids);
    persist(&state);
    let _ = app.emit(names::DOWNLOAD_QUEUE_CHANGED, &serde_json::json!({}));
    Ok(n)
}

/// 扫描剧集目录补登记（任务记录丢了但文件还在时自愈）。
#[tauri::command]
pub fn rescan_downloads(state: State<'_, AppState>, series_id: String) -> AppResult<usize> {
    let settings = state.settings();
    let series = state
        .store
        .read()
        .series(&series_id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("剧集 {series_id}")))?;

    let dir = settings.series_dir(&series.title);
    let n = crate::service::download_service::rescan::adopt_all(&state.queue(), &series, &dir);
    persist(&state);
    Ok(n)
}
