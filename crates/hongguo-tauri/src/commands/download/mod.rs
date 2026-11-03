//! 下载队列 command：本文件是查询（任务列表、队列状态）；变更操作（提交、暂停、重试、删除、补登记）拆在 `actions` / `cleanup` 子模块，编排全在 service 的 `download_service::actions`。

use tauri::State;

use hongguo_common::model::{DownloadTask, QueueStatus};
use hongguo_service::app_state::AppState;

mod actions;
mod cleanup;

pub use actions::*;

pub use cleanup::*;

/// 全部下载任务。
#[tauri::command]
pub fn get_download_tasks(state: State<'_, AppState>) -> Vec<DownloadTask> {
    state.queue().all()
}

/// 队列状态概览。
#[tauri::command]
pub fn get_queue_status(state: State<'_, AppState>) -> QueueStatus {
    state.queue().status()
}
