use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

const MANIFEST: &str =
    "https://github.com/it00021hot/hongguo-desktop/releases/latest/download/latest.json";
const RELEASES: &str = "https://github.com/it00021hot/hongguo-desktop/releases/latest";

pub(crate) fn card(external: bool) -> View {
    let busy = signal(false);
    let checked = signal(false);
    let available = signal(None::<String>);
    let failure = signal(SharedString::default());
    ui::form_surface(
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
                        ui::update_action(
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
                                    if external && !open_url(RELEASES).await {
                                        failure.set(tr("update.releaseOpenFailed"));
                                    }
                                } else {
                                    let result = if external { check().await } else { Ok(None) };
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
            .child(ui::form_hint(move || {
                let version =
                    tr("update.versionLine").replace("{version}", env!("CARGO_PKG_VERSION"));
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
            }))
            .child(when(
                move || available.get().is_some(),
                move || ui::form_description(move || tr("update.nshReleaseNote")),
            ))
            .child(when(
                move || !failure.get().is_empty(),
                move || ui::status(failure),
            )),
    )
}

async fn check() -> Result<Option<String>, String> {
    let response = fetch(MANIFEST).await?;
    if response.status() != 200 {
        return Err(format!("HTTP {}", response.status()));
    }
    newer_version(&response.text(), env!("CARGO_PKG_VERSION"))
}

fn newer_version(manifest: &str, current: &str) -> Result<Option<String>, String> {
    let manifest: serde_json::Value =
        serde_json::from_str(manifest).map_err(|error| error.to_string())?;
    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Missing release version".to_owned())?;
    let candidate = semver::Version::parse(version.trim_start_matches('v'))
        .map_err(|error| error.to_string())?;
    let current = semver::Version::parse(current).map_err(|error| error.to_string())?;
    Ok((candidate > current).then(|| version.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn releases_compare_semantic_versions_without_treating_tauri_assets_as_nsh_installers() {
        assert_eq!(
            newer_version(
                r#"{"version":"0.10.0","platforms":{"darwin-aarch64":{"url":"tauri.app.tar.gz"}}}"#,
                "0.9.9"
            )
            .unwrap(),
            Some("0.10.0".into())
        );
        assert_eq!(
            newer_version(r#"{"version":"0.6.6"}"#, "0.6.6").unwrap(),
            None
        );
        assert_eq!(
            newer_version(r#"{"version":"v0.6.5"}"#, "0.6.6").unwrap(),
            None
        );
        assert_eq!(
            newer_version(r#"{"version":"0.6.6-beta.2"}"#, "0.6.6").unwrap(),
            None
        );
        assert!(newer_version(r#"{"version":"bad"}"#, "0.6.6").is_err());
        assert!(newer_version(r#"{}"#, "0.6.6").is_err());
    }
}
