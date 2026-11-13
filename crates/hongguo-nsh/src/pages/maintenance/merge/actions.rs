use super::super::backend;
use super::{Context, Intent, MergeSnapshot, MergeView, StartRequest};
use crate::{events::SessionHandle, i18n::tr};
use hongguo_common::model::MergeTask;
use nsh_core::prelude::*;

pub(super) fn dispatch(context: Context, model: MergeView, intent: Intent) {
    if model.busy.get_untracked() || model.confirming.get_untracked() {
        return;
    }
    let request = if matches!(&intent, Intent::Start) {
        let Some(request) = model.request() else {
            return;
        };
        Some(request)
    } else {
        None
    };
    if matches!(&intent, Intent::Remove(_)) {
        model.confirming.set(true);
    } else {
        model.busy.set(true);
    }
    spawn(async move {
        if matches!(&intent, Intent::Remove(_)) && !confirm_remove(model).await {
            return;
        }
        model.busy.set(true);
        model.message.set(SharedString::default());
        let result = match context {
            Context::Production(session) => {
                production(
                    session.with(|session| *session),
                    model.files,
                    intent,
                    request,
                )
                .await
            }
            Context::Preview(snapshot) => preview_action(snapshot, intent, request),
        };
        match result {
            Ok(message) => model.message.set(message),
            Err(error) => model.message.set(error),
        }
        model.busy.set(false);
    });
}

async fn confirm_remove(model: MergeView) -> bool {
    let result = nsh_base::alert_dialog()
        .title(tr("merge.removeConfirm"))
        .description(tr("merge.removeDesc"))
        .cancel("cancel", tr("common.cancel"))
        .destructive("remove", tr("common.delete"))
        .show()
        .await;
    model.confirming.set(false);
    match result {
        Ok(answer) => answer.action.as_deref() == Some("remove"),
        Err(error) => {
            model.message.set(error.into());
            false
        }
    }
}

async fn production(
    session: SessionHandle,
    files: Option<crate::file_access::FileAccess>,
    intent: Intent,
    request: Option<StartRequest>,
) -> Result<SharedString, SharedString> {
    let state = session.with(|session| session.state.clone());
    match intent {
        Intent::Start => {
            let request = request.expect("validated merge request");
            backend::blocking(move || {
                hongguo_service::merge_service::start::merge_series(
                    &state,
                    &request.series,
                    &request.output,
                    request.mode,
                )
            })
            .await
            .map_err(|error| error.text())?;
            Ok(tr("merge.started"))
        }
        Intent::Remove(id) => {
            backend::blocking(move || hongguo_service::merge_service::remove_task(&state, &id))
                .await
                .map_err(|error| error.text())?;
            session.notify_merges();
            Ok(SharedString::default())
        }
        Intent::Reveal(id) => {
            let path = backend::output_path(state, id)
                .await
                .map_err(|error| error.text())?;
            crate::file_access::reveal_or_export(files, path)
                .await
                .map_err(SharedString::from)
        }
    }
}

fn preview_action(
    snapshot: Signal<MergeSnapshot>,
    intent: Intent,
    request: Option<StartRequest>,
) -> Result<SharedString, SharedString> {
    match intent {
        Intent::Start => {
            let request = request.expect("validated preview merge request");
            let mut task = MergeTask::new(
                &request.series,
                &request.output,
                &request.output,
                request.mode,
            );
            task.id = format!(
                "offline-started-{}",
                snapshot.with(|snapshot| snapshot.tasks.len())
            );
            task.created_at = 1_794_384_000_000;
            task.status = hongguo_common::model::MergeStatus::Running;
            snapshot.update(|snapshot| snapshot.tasks.insert(0, task));
            Ok(tr("merge.started"))
        }
        Intent::Remove(id) => {
            snapshot.update(|snapshot| snapshot.tasks.retain(|task| task.id != id));
            Ok(SharedString::default())
        }
        Intent::Reveal(_) => Ok(tr("nsh.previewAction")
            .replace(
                "{action}",
                &tr(if is_desktop() {
                    "merge.actions.openOutput"
                } else {
                    "nsh.exportFile"
                }),
            )
            .into()),
    }
}
