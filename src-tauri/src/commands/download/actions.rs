//! 下载任务的变更操作：提交、暂停、重试、删除、补登记。
//!
//! 查询类在 mod.rs，这里只放会改状态的操作。

use tauri::{AppHandle, Emitter, State};

use super::names;
use super::persist;
use crate::app_state::AppState;
use crate::domain::model::DownloadTask;
use crate::error::{AppError, AppResult};

/// 提交一批下载任务。
///
/// 返回入队后的任务数。任务真正执行由 [`crate::service::download_service::worker`]
/// 的调度循环完成，进度通过 `download-*` 事件推送。
#[tauri::command]
pub fn download_batch(
    app: AppHandle,
    state: State<'_, AppState>,
    series_id: String,
    vids: Vec<u32>,
) -> AppResult<usize> {
    if vids.is_empty() {
        return Err(AppError::InvalidArgs("没有选择任何集数".into()));
    }

    let series = state
        .store
        .read()
        .series(&series_id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("剧集 {series_id}")))?;

    let mut added = 0usize;
    for vid_index in vids {
        let Some(episode) = series.episodes.iter().find(|e| e.vid_index == vid_index) else {
            continue;
        };
        let task = DownloadTask::new(
            &series.series_id,
            &series.title,
            episode.vid_index,
            &episode.vid,
            &episode.title,
        );
        if state.queue().enqueue(task.clone()).id != task.id {
            continue; // 已在队列里
        }
        added += 1;

        let _ = app.emit(
            names::DOWNLOAD_TASK_ADDED,
            &serde_json::to_value(&task).unwrap_or_default(),
        );
    }

    persist(&state);
    crate::service::download_service::kick_from_handle(&app);
    Ok(added)
}

/// 下载单集。
#[tauri::command]
pub fn download_single_episode(
    app: AppHandle,
    state: State<'_, AppState>,
    series_id: String,
    vid_index: u32,
) -> AppResult<usize> {
    download_batch(app, state, series_id, vec![vid_index])
}

/// 一键暂停：取消进行中的任务并清空待运行队列。
#[tauri::command]
pub fn pause_all(app: AppHandle, state: State<'_, AppState>) -> AppResult<usize> {
    // 队列与调度器都要停：只停队列的话调度器仍会把已派出的任务跑完，
    // 用户点「全部暂停」后进度条还在走。
    state.scheduler().pause_all();
    let n = state.queue().pause_all();
    persist(&state);
    let _ = app.emit(names::DOWNLOAD_QUEUE_CHANGED, &serde_json::json!({}));
    Ok(n)
}

/// 一键启动：把未完成任务重新排队。
#[tauri::command]
pub fn resume_all(app: AppHandle, state: State<'_, AppState>) -> AppResult<usize> {
    state.scheduler().resume_all();
    let n = state.queue().resume_all();
    persist(&state);
    let _ = app.emit(names::DOWNLOAD_QUEUE_CHANGED, &serde_json::json!({}));
    crate::service::download_service::kick_from_handle(&app);
    Ok(n)
}

/// 停止单个任务。
#[tauri::command]
pub fn stop_download(app: AppHandle, state: State<'_, AppState>, task_id: String) -> AppResult<()> {
    if state.queue().get(&task_id).is_none() {
        return Err(AppError::NotFound(format!("任务 {task_id}")));
    }
    state.queue().mark_stopped(&task_id);
    persist(&state);
    let _ = app.emit(
        names::DOWNLOAD_STOPPED,
        &serde_json::json!({ "id": task_id }),
    );
    Ok(())
}

/// 重试单个任务。
#[tauri::command]
pub fn retry_task(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<DownloadTask> {
    let task = state
        .queue()
        .retry(&task_id)
        .ok_or_else(|| AppError::NotFound(format!("任务 {task_id}")))?;
    persist(&state);
    let _ = app.emit(names::DOWNLOAD_QUEUE_CHANGED, &serde_json::json!({}));
    Ok(task)
}

/// 批量重试。
#[tauri::command]
pub fn retry_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    task_ids: Vec<String>,
) -> AppResult<usize> {
    let n = state.queue().retry_many(&task_ids);
    persist(&state);
    let _ = app.emit(names::DOWNLOAD_QUEUE_CHANGED, &serde_json::json!({}));
    Ok(n)
}
