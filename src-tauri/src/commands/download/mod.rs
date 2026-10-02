//! 下载队列 command。
//!
//! 按职责拆分：
//! - 本文件：查询（任务列表、队列状态）+ 落盘辅助
//! - [`actions`]：变更（提交、暂停、重试、删除）

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::{DownloadTask, QueueStatus};
use crate::error::AppResult;

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

/// 把队列快照写回数据文件。
pub(crate) fn persist(state: &State<'_, AppState>) -> AppResult<()> {
    crate::store::persist_tasks(state, "Download")
}
