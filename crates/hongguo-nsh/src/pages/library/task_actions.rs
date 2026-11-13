use super::tasks::TaskIntent;
use crate::{bridge, i18n::tr};
use hongguo_common::AppError;
use hongguo_service::{AppState, download_service::actions};
use nsh_core::prelude::*;

#[cfg(test)]
mod export_tests;

pub(super) async fn run(
    state: AppState,
    intent: TaskIntent,
    files: Option<crate::file_access::FileAccess>,
) -> Result<SharedString, bridge::Failure> {
    if let TaskIntent::Folder(task_id) = intent {
        let path =
            bridge::request(async move { downloaded_file(&state.queue(), &task_id) }).await?;
        return crate::file_access::reveal_or_export(files, path)
            .await
            .map_err(|error| AppError::Io(error).into());
    }
    let result = bridge::request(async move {
        hongguo_service::runtime::spawn_blocking(move || match intent {
            TaskIntent::Pause => actions::pause_all(&state).map(|_| None),
            TaskIntent::Resume => actions::resume_all(&state).map(|_| None),
            TaskIntent::Stop(id) => actions::stop_task(&state, &id).map(|_| None),
            TaskIntent::Retry(ids) => actions::retry_tasks(&state, &ids).map(|_| None),
            TaskIntent::Delete(ids, files) => {
                actions::delete_tasks(&state, &ids, files).map(|_| None)
            }
            TaskIntent::Rescan => actions::rescan_downloads(&state).map(Some),
            TaskIntent::Folder(_) | TaskIntent::Remove(_) => {
                Err(AppError::InvalidArgs("unconfirmed download action".into()))
            }
        })
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
    })
    .await?;
    Ok(match result {
        Some(0) => tr("tasks.rescanNone"),
        Some(count) => tr("tasks.rescanFound")
            .replace("{count}", &count.to_string())
            .into(),
        None => SharedString::default(),
    })
}

fn downloaded_file(
    queue: &hongguo_service::download_service::queue::DownloadQueue,
    task_id: &str,
) -> hongguo_common::AppResult<std::path::PathBuf> {
    queue
        .get(task_id)
        .filter(|task| {
            task.status == hongguo_common::model::TaskStatus::Completed
                && !task.file_path.is_empty()
        })
        .map(|task| task.file_path.into())
        .ok_or_else(|| AppError::NotFound(format!("task {task_id} has no completed file")))
}
