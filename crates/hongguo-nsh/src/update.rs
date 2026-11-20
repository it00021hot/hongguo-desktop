use crate::{i18n::tr, ui};
use nsh_core::prelude::*;
use std::future::Future;

mod desktop;
const RELEASES: &str = "https://github.com/it00021hot/hongguo-desktop/releases/latest";

#[derive(Clone, Copy)]
struct Updates {
    owner: Owner,
    busy: Signal<bool>,
    checked: Signal<bool>,
    available: Signal<Option<nsh_updater::Available>>,
    downloaded: Signal<Option<std::path::PathBuf>>,
    operation: Signal<&'static str>,
    failure: Signal<SharedString>,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            owner: Owner::current(),
            busy: signal(false),
            checked: signal(false),
            available: signal(None),
            downloaded: signal(None),
            operation: signal("update.checking"),
            failure: signal(SharedString::default()),
        }
    }
}

pub(crate) fn check_on_startup() {
    if is_desktop() {
        let updates = global::<Updates>().get_untracked();
        if !updates.checked.get_untracked() {
            updates.check();
        }
    }
}

impl Updates {
    fn check(self) {
        self.run("update.checking", async move {
            let update = desktop::check().await?;
            self.available.set(update);
            self.checked.set(true);
            Ok(())
        });
    }

    fn activate(self) {
        if !is_desktop() {
            self.run("update.checking", async {
                if open_url(RELEASES).await {
                    Ok(())
                } else {
                    Err(tr("update.releaseOpenFailed").to_string())
                }
            });
        } else if let Some(path) = self.downloaded.get_untracked() {
            self.run("update.installing", async move {
                let update = self
                    .available
                    .get_untracked()
                    .ok_or("missing update metadata")?;
                desktop::install(path, update).await
            });
        } else if let Some(update) = self.available.get_untracked() {
            self.run("update.downloadingArchive", async move {
                let path = desktop::download(update).await?;
                self.downloaded.set(Some(path));
                Ok(())
            });
        } else {
            self.check();
        }
    }

    fn run(
        self,
        operation: &'static str,
        work: impl Future<Output = Result<(), String>> + 'static,
    ) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.failure.set(SharedString::default());
        self.operation.set(operation);
        self.owner.run(move || {
            spawn(async move {
                let start = nsh_core::time::now();
                log::info!("[NSH update] operation started operation={operation}");
                let result = work.await;
                let elapsed_ms = start.elapsed().as_millis();
                match result {
                    Ok(()) => log::info!(
                        "[NSH update] operation completed operation={operation} elapsedMs={elapsed_ms}"
                    ),
                    Err(error) => {
                        log::error!(
                            "[NSH update] operation failed operation={operation} elapsedMs={elapsed_ms}: {error}"
                        );
                        let message = tr(match operation {
                            "update.installing" => "update.installFailed",
                            "update.downloadingArchive" => "update.downloadFailed",
                            _ => "update.checkFailed",
                        });
                        self.failure.set(message.replace("{error}", &error).into());
                    }
                }
                self.busy.set(false);
            });
        });
    }
}

pub(crate) fn card() -> View {
    let updates = global::<Updates>().get_untracked();
    let Updates {
        busy,
        checked,
        available,
        downloaded,
        operation,
        failure,
        ..
    } = updates;
    crate::features::settings::presentation::form_surface(
        view()
            .gap_1p5()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        text(move || tr("update.appTitle"))
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(view().flex_1())
                    .child(
                        crate::features::settings::presentation::update_action(
                            move || {
                                tr(if busy.get() {
                                    operation.get()
                                } else if downloaded.get().is_some() {
                                    "update.installNow"
                                } else if available.get().is_some() {
                                    "update.download"
                                } else {
                                    "update.checkUpdate"
                                })
                            },
                            computed(move || available.get().is_some()),
                        )
                        .disabled(busy)
                        .on_press(move || updates.activate()),
                    ),
            )
            .child(crate::features::settings::presentation::form_hint(
                move || {
                    let version =
                        tr("update.versionLine").replace("{version}", &crate::metadata().version);
                    if let Some(available) = available.get() {
                        format!(
                            "{version} · {}",
                            tr("update.newVersionAvailable")
                                .replace("{version}", &available.version)
                        )
                    } else if checked.get() {
                        format!("{version} · {}", tr("update.upToDate"))
                    } else {
                        version
                    }
                },
            ))
            .child(when(
                move || available.get().is_some(),
                move || {
                    crate::features::settings::presentation::form_description(move || {
                        if downloaded.get().is_some() {
                            tr("update.downloadedNote")
                        } else if let Some(notes) = available
                            .get()
                            .and_then(|update| update.notes)
                            .map(|notes| notes.trim().to_owned())
                            .filter(|notes| !notes.is_empty())
                        {
                            notes.into()
                        } else {
                            tr("update.nshReleaseNote")
                        }
                    })
                },
            ))
            .child(when(
                move || !failure.get().is_empty(),
                move || ui::status(failure),
            )),
    )
}
