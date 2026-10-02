//! 合并任务。
//!
//! 两种模式：快速合并（流复制，不重编码）与兼容合并（转 H.264）。

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::domain::model::{MergeMode, MergePreflight, MergeStatus, MergeTask};
use crate::error::{AppError, AppResult};
use crate::service::download_service::events::names;
use crate::service::merge_service::{compat, prepare, progress, quick, remove_task};

/// 全部合并任务。
#[tauri::command]
pub fn get_merge_tasks(state: State<'_, AppState>) -> Vec<MergeTask> {
    state.store.read().merge_tasks.clone()
}

/// 删除一条合并任务记录（只删记录，不删已产出的文件）。
#[tauri::command]
pub fn delete_merge_task(state: State<'_, AppState>, id: String) -> AppResult<()> {
    remove_task(&state, &id)
}

/// 合并前校验。
#[tauri::command]
pub fn merge_preflight(state: State<'_, AppState>, series_id: String) -> AppResult<MergePreflight> {
    prepare::preflight(&state, &series_id)
}

/// 启动合并。
///
/// 合并是**重活**（兼容合并要逐集转码），这里同步执行并返回结果，
/// 进度通过 `merge-progress` 事件推送：每完成一集发一次，
/// 收尾再发一次 `merge-completed` / `merge-failed`。
#[tauri::command]
pub fn merge_series(
    app: AppHandle,
    state: State<'_, AppState>,
    series_id: String,
    output_name: String,
    mode: MergeMode,
) -> AppResult<MergeTask> {
    let series_title = state
        .store
        .read()
        .series(&series_id)
        .map(|s| s.title.clone())
        .unwrap_or_else(|| output_name.clone());

    let mut task = MergeTask::new(&series_id, &series_title, &output_name, mode);
    task.status = MergeStatus::Running;
    upsert_in_memory(&state, &task);
    // 合并可能跑好几分钟，先广播「开始了」，UI 立刻能看到 running 状态
    let _ = app.emit(names::MERGE_TASK_ADDED, &task);

    let emit_app = app.clone();
    let on_progress: progress::ProgressSink = Arc::new(move |done, total, t| {
        // 直接发填好 percent 的 MergeTask：前端已有的 schema 里就有这个字段，
        // 不必为进度另造一套载荷类型。
        let mut snapshot = t.clone();
        snapshot.percent = if total == 0 {
            0.0
        } else {
            (done as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
        };
        let _ = emit_app.emit(names::MERGE_PROGRESS, snapshot);
    });

    let inner = state.inner().clone();
    let result = match mode {
        MergeMode::Quick => {
            quick::quick_merge(&state, &series_id, &output_name, &task, &on_progress)
        }
        MergeMode::Compat => {
            compat::compat_merge(&inner, &series_id, &output_name, &task, &on_progress)
        }
    };

    match result {
        Ok((path, size, count)) => {
            task.mark_completed(&path.to_string_lossy(), size);
            task.episode_count = count;
        }
        Err(e) => {
            // 与下载任务同一个约定：记录里存 i18n key（前端会 t() 它），
            // 内部细节只进日志。直接把 e.to_string() 存进去等于把后端术语糊给用户。
            log::error!("[Merge] {} 合并失败: {e}", task.output_name);
            task.mark_failed(e.i18n_key());
        }
    }
    task.status = if task.error.is_empty() {
        MergeStatus::Completed
    } else {
        MergeStatus::Failed
    };

    // 覆盖刚才那条 pending/running 记录，这次连同结果一起落盘
    upsert_in_memory(&state, &task);
    state
        .store
        .write()
        .save(&crate::store::paths::data_file())
        .map_err(|e| AppError::StoreCorrupt(e.to_string()))?;

    let _ = app.emit(
        if task.status == MergeStatus::Failed {
            names::MERGE_FAILED
        } else {
            names::MERGE_COMPLETED
        },
        &task,
    );

    if task.status == MergeStatus::Failed {
        return Err(AppError::Media(task.error.clone()));
    }
    Ok(task)
}

/// 把任务写进内存里的合并列表（不落盘）。
///
/// 开始时先插一条 running 记录让 UI 立刻看到任务；跑完后再用同一条覆盖它。
fn upsert_in_memory(state: &State<'_, AppState>, task: &MergeTask) {
    let mut data = state.store.write();
    data.merge_tasks.retain(|t| t.id != task.id);
    data.merge_tasks.push(task.clone());
}
