use super::model::{StorageRow, StorageSnapshot};
use crate::bridge;
use hongguo_common::{AppError, AppResult};
use hongguo_service::{app_state::AppState, merge_service, storage_service};

pub(super) fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> bridge::Request<T> {
    bridge::request(async move {
        hongguo_service::runtime::spawn_blocking(work)
            .await
            .map_err(|error| AppError::Internal(format!("NSH maintenance task: {error}")))?
    })
}

pub(super) fn candidates(
    state: AppState,
) -> bridge::Request<Vec<hongguo_common::model::MergeCandidate>> {
    blocking(move || Ok(merge_service::candidates(&state)))
}

pub(super) fn tasks(state: AppState) -> bridge::Request<Vec<hongguo_common::model::MergeTask>> {
    blocking(move || state.store.merge_tasks())
}

pub(super) fn storage(state: AppState) -> bridge::Request<StorageSnapshot> {
    blocking(move || {
        let usage = storage_service::usage::usage(&state);
        Ok(StorageSnapshot {
            bytes: usage.bytes,
            files: usage.files,
            rows: storage_service::usage::series_usage(&state)
                .into_iter()
                .map(|row| StorageRow {
                    id: row.series_id,
                    title: row.title,
                    bytes: row.bytes,
                    files: row.files,
                })
                .collect(),
        })
    })
}

pub(super) fn output_path(state: AppState, id: String) -> bridge::Request<std::path::PathBuf> {
    blocking(move || {
        state
            .store
            .merge_tasks()?
            .into_iter()
            .find(|task| task.id == id)
            .map(|task| task.output_path)
            .filter(|path| !path.trim().is_empty())
            .map(std::path::PathBuf::from)
            .ok_or_else(|| AppError::NotFound(format!("合并任务 {id} 没有产物路径")))
    })
}
