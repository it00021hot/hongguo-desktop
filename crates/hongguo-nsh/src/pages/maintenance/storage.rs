use super::{backend, fixtures, model::StorageSnapshot};
use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;

mod view;

#[derive(Clone)]
pub(super) enum Intent {
    Series(String, String),
    All,
}

pub(in crate::pages) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    let completed = super::completed_inputs(session);
    let usage: Resource<StorageSnapshot, bridge::Failure> = resource(
        move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&Route::Storage))
                .then(|| (session.identity(), completed.get()))
        },
        move |_, _| backend::storage(session.with(|session| session.state.clone())),
    );
    let busy = signal(false);
    let confirming = signal(false);
    let message = signal(SharedString::default());
    let data = computed(move || usage.get());
    let act = move |intent: Intent| {
        if busy.get_untracked() || confirming.get_untracked() {
            return;
        }
        match &intent {
            Intent::All => confirming.set(true),
            Intent::Series(..) => busy.set(true),
        }
        spawn(async move {
            if matches!(&intent, Intent::All) && !confirm_all(confirming, message).await {
                return;
            }
            busy.set(true);
            message.set(SharedString::default());
            let state = session.with(|session| session.state.clone());
            let title = match &intent {
                Intent::Series(_, title) => Some(title.clone()),
                Intent::All => None,
            };
            let outcome = backend::blocking(move || match intent {
                Intent::Series(id, _) => {
                    hongguo_service::storage_service::cleanup::delete_series(&state, &id)
                }
                Intent::All => hongguo_service::storage_service::cleanup::delete_all(&state),
            })
            .await;
            match outcome {
                Ok(files) => {
                    message.set(freed(files, title.as_deref()));
                }
                Err(error) => message.set(error.text()),
            }
            // Cleanup may have removed files before a later persistence error.
            session.notify_downloads();
            session.notify_library();
            usage.refetch();
            busy.set(false);
        });
    };
    ui::page(nav, move || tr("settings.storage")).child(
        view::content(data, busy, confirming, message, act).child(when(
            move || usage.error().is_some(),
            move || {
                crate::pages::error_state(
                    move || {
                        usage
                            .error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || usage.refetch(),
                )
            },
        )),
    )
}

pub(in crate::pages) fn preview(nav: Navigation) -> Screen {
    let snapshot = signal(fixtures::storage());
    let data = computed(move || Some(snapshot.get()));
    let busy = signal(false);
    let confirming = signal(false);
    let message = signal(SharedString::default());
    ui::page(nav, move || tr("settings.storage")).child(view::content(
        data,
        busy,
        confirming,
        message,
        move |intent| {
            if busy.get_untracked() || confirming.get_untracked() {
                return;
            }
            match &intent {
                Intent::All => confirming.set(true),
                Intent::Series(..) => busy.set(true),
            }
            spawn(async move {
                if matches!(&intent, Intent::All) && !confirm_all(confirming, message).await {
                    return;
                }
                let (id, title) = match &intent {
                    Intent::Series(id, title) => (Some(id.as_str()), Some(title.as_str())),
                    Intent::All => (None, None),
                };
                let removed = snapshot.with(|snapshot| {
                    snapshot
                        .rows
                        .iter()
                        .filter(|row| id.is_none_or(|id| row.id == id))
                        .map(|row| row.files)
                        .sum()
                });
                snapshot.update(|snapshot| {
                    snapshot.remove(id);
                });
                message.set(freed(removed, title));
                busy.set(false);
            });
        },
    ))
}

async fn confirm_all(confirming: Signal<bool>, message: Signal<SharedString>) -> bool {
    confirming.set(true);
    let answer = nsh_base::alert_dialog()
        .title(tr("storage.deleteAllConfirm"))
        .description(tr("storage.deleteAllDesc"))
        .cancel("cancel", tr("common.cancel"))
        .destructive("clear", tr("common.confirm"))
        .show()
        .await;
    confirming.set(false);
    match answer {
        Ok(answer) => answer.action.as_deref() == Some("clear"),
        Err(error) => {
            message.set(error.into());
            false
        }
    }
}

fn freed(files: usize, title: Option<&str>) -> SharedString {
    let freed = tr("storage.freedFiles").replace("{count}", &files.to_string());
    title
        .map_or_else(|| freed.clone(), |title| format!("{title} · {freed}"))
        .into()
}
