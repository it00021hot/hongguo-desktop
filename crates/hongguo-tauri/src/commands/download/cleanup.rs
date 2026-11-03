//! 下载任务的删除与补登记——都是「动了文件」的操作，放一起便于审查副作用；编排（删文件 + 状态机 + 事件 + 落盘）全在 service 的 `download_service::actions`，这里只做参数转调。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_service::app_state::AppState;
use hongguo_service::download_service::actions;

/// 删除任务；`delete_files` 为真时同时删除本地文件。
#[tauri::command]
pub fn delete_tasks(
    state: State<'_, AppState>,
    task_ids: Vec<String>,
    delete_files: Option<bool>,
) -> AppResult<usize> {
    actions::delete_tasks(state.inner(), &task_ids, delete_files.unwrap_or(false))
}

/// 扫描下载目录，把磁盘上有文件但任务记录丢了的集补登记为已完成，返回补回的条数。已有记录的集（无论状态）不动。
#[tauri::command]
pub fn rescan_downloads(state: State<'_, AppState>) -> AppResult<usize> {
    actions::rescan_downloads(state.inner())
}
