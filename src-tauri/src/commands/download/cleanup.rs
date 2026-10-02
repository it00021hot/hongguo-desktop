//! 下载任务的删除。
//!
//! 删除与补登记都是「动了文件」的操作，放一起便于审查副作用。

use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::error::AppResult;

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
        let mut failed: Vec<String> = Vec::new();
        for id in &task_ids {
            if let Some(task) = state.queue().get(id) {
                if task.is_done() {
                    let path = std::path::Path::new(&task.file_path);
                    if let Err(e) = std::fs::remove_file(path) {
                        failed.push(format!("{} ({e})", path.display()));
                    }
                    crate::service::download_service::worker::cleanup_temp(path);
                }
            }
        }
        // 删不掉的文件（被播放器占用、权限不足）必须留痕：否则用户看到
        // 「已删除」却发现文件还在，只能靠猜。
        if !failed.is_empty() {
            log::warn!(
                "[Download] {} 个文件删除失败: {}",
                failed.len(),
                failed.join("；")
            );
        }
    }

    let n = state.queue().remove(&task_ids);
    persist(&state)?;
    let _ = app.emit(names::DOWNLOAD_QUEUE_CHANGED, &serde_json::json!({}));
    Ok(n)
}
