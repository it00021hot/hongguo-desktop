//! 下载任务的变更入口：入队 / 暂停 / 恢复 / 停止 / 重试 / 删除 / 重扫。
//! 编排只有这一份（状态机变更 + 落盘 + 事件发布 + 触发调度）：Tauri 命令
//! 与 GPUI 面板都调这里，壳不再直接操作队列或自行发布下载事件。

use hongguo_common::model::{DownloadTask, TaskStatus};
use hongguo_common::{AppError, AppResult};

use crate::app_state::AppState;
use crate::download_service::events::{QueueChangedEvent, TaskEvent};
use crate::download_service::{kick, persist_tasks};
use crate::events::ServiceEvent;

/// 提交一批下载任务（在剧集档案里按集号找分集），返回新入队数。
/// 已在队列里的集自动跳过（`enqueue` 去重）；每个新任务发 `download-task-added`，随后触发一轮调度。
pub fn enqueue_batch(state: &AppState, series_id: &str, vids: &[u32]) -> AppResult<usize> {
    if vids.is_empty() {
        return Err(AppError::InvalidArgs("没有选择任何集数".into()));
    }
    let series = state
        .store
        .series_by_id(series_id)?
        .ok_or_else(|| AppError::NotFound(format!("剧集 {series_id}")))?;

    let mut added = 0usize;
    for vid_index in vids {
        let Some(episode) = series.episodes.iter().find(|e| e.vid_index == *vid_index) else {
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
        state
            .events()
            .publish(ServiceEvent::DownloadTaskAdded(task));
    }

    persist_tasks(&state.store, &state.queue(), "Download")?;
    kick(state.clone());
    Ok(added)
}

/// 一键暂停：取消在途任务并清空待运行队列。
/// 队列与调度器都要停：只停队列的话调度器仍会把已派出的任务跑完。
pub fn pause_all(state: &AppState) -> AppResult<usize> {
    state.scheduler().pause_all();
    let n = state.queue().pause_all();
    persist_tasks(&state.store, &state.queue(), "Download")?;
    state
        .events()
        .publish(ServiceEvent::DownloadQueueChanged(QueueChangedEvent {}));
    Ok(n)
}

/// 一键启动：把未完成任务重新排队并触发调度。
pub fn resume_all(state: &AppState) -> AppResult<usize> {
    state.scheduler().resume_all();
    let n = state.queue().resume_all();
    persist_tasks(&state.store, &state.queue(), "Download")?;
    state
        .events()
        .publish(ServiceEvent::DownloadQueueChanged(QueueChangedEvent {}));
    kick(state.clone());
    Ok(n)
}

/// 停止单个任务（真实取消：在途任务在下个分片检查点退出）。
/// 在途任务的 `download-stopped` 由调度器收尾时发布（避免双事件）；尚未在跑的任务这里立即发。
pub fn stop_task(state: &AppState, task_id: &str) -> AppResult<()> {
    let Some(task) = state.queue().get(task_id) else {
        return Err(AppError::NotFound(format!("任务 {task_id}")));
    };
    let was_running = task.status == TaskStatus::Running;
    state.scheduler().cancel_task(task_id);
    state.queue().mark_stopped(task_id);
    persist_tasks(&state.store, &state.queue(), "Download")?;
    if !was_running {
        state
            .events()
            .publish(ServiceEvent::DownloadStopped(TaskEvent {
                id: task_id.to_string(),
            }));
    }
    Ok(())
}

/// 重试单个任务（重置后立即触发调度）。
pub fn retry_task(state: &AppState, task_id: &str) -> AppResult<DownloadTask> {
    let task = state
        .queue()
        .retry(task_id)
        .ok_or_else(|| AppError::NotFound(format!("任务 {task_id}")))?;
    persist_tasks(&state.store, &state.queue(), "Download")?;
    state
        .events()
        .publish(ServiceEvent::DownloadQueueChanged(QueueChangedEvent {}));
    kick(state.clone());
    Ok(task)
}

/// 批量重试（重置后立即触发调度）。
pub fn retry_tasks(state: &AppState, task_ids: &[String]) -> AppResult<usize> {
    let n = state.queue().retry_many(task_ids);
    persist_tasks(&state.store, &state.queue(), "Download")?;
    state
        .events()
        .publish(ServiceEvent::DownloadQueueChanged(QueueChangedEvent {}));
    kick(state.clone());
    Ok(n)
}

/// 删除任务；`delete_files` 为真时同时删除本地文件。
pub fn delete_tasks(state: &AppState, task_ids: &[String], delete_files: bool) -> AppResult<usize> {
    if delete_files {
        // 逐个删文件（删任务与删文件是两个独立操作，这里按用户勾选联动）
        let mut failed: Vec<String> = Vec::new();
        for id in task_ids {
            if let Some(task) = state.queue().get(id)
                && task.is_done()
            {
                let path = std::path::Path::new(&task.file_path);
                if let Err(e) = hongguo_store::paths::qa_remove_file(path) {
                    failed.push(format!("{} ({e})", path.display()));
                }
                crate::download_service::worker::cleanup_temp(path);
            }
        }
        // 删不掉的文件（被播放器占用、权限不足）必须留痕，否则用户「已删除」了文件却还在。
        if !failed.is_empty() {
            log::warn!(
                "[Download] {} 个文件删除失败: {}",
                failed.len(),
                failed.join("；")
            );
        }
    }

    let n = state.queue().remove(task_ids);
    persist_tasks(&state.store, &state.queue(), "Download")?;
    state
        .events()
        .publish(ServiceEvent::DownloadQueueChanged(QueueChangedEvent {}));
    Ok(n)
}

/// 扫描下载目录，把磁盘上有文件但任务记录丢了的集补登记为已完成，返回补回的条数。
/// 已有记录的集（无论状态）不动；补回的每条发 `download-task-added`（有补回时补发队列变化信号）。
pub fn rescan_downloads(state: &AppState) -> AppResult<usize> {
    let summary = crate::download_service::rescan::rescan_from_disk(state)?;
    for task in &summary.added {
        state
            .events()
            .publish(ServiceEvent::DownloadTaskAdded(task.clone()));
    }
    if !summary.added.is_empty() {
        state
            .events()
            .publish(ServiceEvent::DownloadQueueChanged(QueueChangedEvent {}));
    }
    log::info!(
        "[Rescan] 扫过 {} 部剧，补回 {} 条记录",
        summary.scanned_series,
        summary.added.len()
    );
    Ok(summary.added.len())
}
