use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

const RELEASE_API: &str = "https://api.github.com/repos/it00021hot/hongguo-desktop/releases/latest";
const RELEASES: &str = "https://github.com/it00021hot/hongguo-desktop/releases/latest";

#[derive(serde::Deserialize)]
struct Release {
    #[serde(rename = "tag_name")]
    version: String,
}

pub(crate) fn card() -> View {
    let busy = signal(false);
    let checked = signal(false);
    let available = signal(None::<String>);
    let failure = signal(SharedString::default());
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
                                    "update.checking"
                                } else if available.get().is_some() {
                                    "update.viewNewVersion"
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
                            spawn(async move {
                                if available.get_untracked().is_some() {
                                    if !open_url(RELEASES).await {
                                        failure.set(tr("update.releaseOpenFailed"));
                                    }
                                } else {
                                    let result = check().await;
                                    match result {
                                        Ok(version) => {
                                            available.set(version);
                                            checked.set(true);
                                        }
                                        Err(error) => failure.set(
                                            tr("update.checkFailed")
                                                .replace("{error}", &error)
                                                .into(),
                                        ),
                                    }
                                }
                                busy.set(false);
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
                            tr("update.newVersionAvailable").replace("{version}", &available)
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
                        tr("update.nshReleaseNote")
                    })
                },
            ))
            .child(when(
                move || !failure.get().is_empty(),
                move || ui::status(failure),
            )),
    )
}

async fn check() -> Result<Option<String>, String> {
    let request = HttpRequest::get(RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", format!("guava/{}", crate::metadata().version));
    let response = fetch(request).await?;
    if response.status() != 200 {
        return Err(format!("HTTP {}", response.status()));
    }
    newer_version(&response.text(), &crate::metadata().version)
}

fn newer_version(response: &str, current: &str) -> Result<Option<String>, String> {
    let release: Release = serde_json::from_str(response).map_err(|error| error.to_string())?;
    let candidate = semver::Version::parse(release.version.trim_start_matches('v'))
        .map_err(|error| error.to_string())?;
    let current = semver::Version::parse(current).map_err(|error| error.to_string())?;
    Ok((candidate > current).then_some(release.version))
}
