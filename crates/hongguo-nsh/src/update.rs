use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

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

pub(crate) fn card() -> View {
    let Updates {
        owner,
        busy,
        checked,
        available,
        downloaded,
        operation,
        failure,
    } = global::<Updates>().get_untracked();
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
                        .on_press(move || {
                            if busy.get_untracked() {
                                return;
                            }
                            busy.set(true);
                            failure.set(SharedString::default());
                            operation.set(if downloaded.get_untracked().is_some() {
                                "update.installing"
                            } else if available.get_untracked().is_some() {
                                "update.downloadingArchive"
                            } else {
                                "update.checking"
                            });
                            owner.run(move || {
                                spawn(async move {
                                    let result = if !is_desktop() {
                                        if open_url(RELEASES).await {
                                            Ok(())
                                        } else {
                                            Err(tr("update.releaseOpenFailed").to_string())
                                        }
                                    } else if let Some(path) = downloaded.get_untracked() {
                                        match available.get_untracked() {
                                            Some(update) => desktop::install(path, update).await,
                                            None => Err("missing update metadata".into()),
                                        }
                                    } else if let Some(update) = available.get_untracked() {
                                        desktop::download(update)
                                            .await
                                            .map(|path| downloaded.set(Some(path)))
                                    } else {
                                        desktop::check().await.map(|update| {
                                            available.set(update);
                                            checked.set(true);
                                        })
                                    };
                                    if let Err(error) = result {
                                        log::error!("[NSH update] operation failed: {error}");
                                        failure.set(
                                            tr(match operation.get_untracked() {
                                                "update.installing" => "update.installFailed",
                                                "update.downloadingArchive" => {
                                                    "update.downloadFailed"
                                                }
                                                _ => "update.checkFailed",
                                            })
                                            .replace("{error}", &error)
                                            .into(),
                                        );
                                    }
                                    busy.set(false);
                                });
                            });
                        }),
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
