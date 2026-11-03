//! 下载任务的变更操作：提交、暂停、重试。查询类在 mod.rs；编排（状态机 + 落盘 + 事件 + 调度）全在 service 的 `download_service::actions`，这里只做参数转调。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_common::model::DownloadTask;
use hongguo_service::app_state::AppState;
use hongguo_service::download_service::actions;

/// 提交一批下载任务，返回入队后的任务数；进度与状态经 `download-*` 事件推送（service 发布）。
#[tauri::command]
pub fn download_batch(
    state: State<'_, AppState>,
    series_id: String,
    vids: Vec<u32>,
) -> AppResult<usize> {
    actions::enqueue_batch(state.inner(), &series_id, &vids)
}

/// 一键暂停：取消进行中的任务并清空待运行队列。
#[tauri::command]
pub fn pause_all(state: State<'_, AppState>) -> AppResult<usize> {
    actions::pause_all(state.inner())
}

/// 一键启动：把未完成任务重新排队。
#[tauri::command]
pub fn resume_all(state: State<'_, AppState>) -> AppResult<usize> {
    actions::resume_all(state.inner())
}

/// 停止单个任务。
#[tauri::command]
pub fn stop_download(state: State<'_, AppState>, task_id: String) -> AppResult<()> {
    actions::stop_task(state.inner(), &task_id)
}

/// 重试单个任务。
#[tauri::command]
pub fn retry_task(state: State<'_, AppState>, task_id: String) -> AppResult<DownloadTask> {
    actions::retry_task(state.inner(), &task_id)
}

/// 批量重试。
#[tauri::command]
pub fn retry_tasks(state: State<'_, AppState>, task_ids: Vec<String>) -> AppResult<usize> {
    actions::retry_tasks(state.inner(), &task_ids)
}
