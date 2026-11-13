use super::*;
use nsh_core::platform::Os;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

const PRIVATE_PATH: &str = "/private/application-container/Library/Application Support/downloads";

#[component]
fn Directories() -> impl IntoElement {
    let draft = signal(Draft::from(hongguo_common::model::Settings {
        download_dir: PRIVATE_PATH.into(),
        cache_dir: PRIVATE_PATH.into(),
        ..Default::default()
    }));
    let busy = signal(false);
    let message = signal(SharedString::default());
    view()
        .child(field(
            "settings.downloadDir",
            draft,
            busy,
            message,
            false,
            |draft| draft.download_dir.clone(),
            |draft, value| draft.download_dir = value,
        ))
        .child(field(
            "settings.cacheDir",
            draft,
            busy,
            message,
            false,
            |draft| draft.cache_dir.clone(),
            |draft, value| draft.cache_dir = value,
        ))
}

#[test]
fn mobile_directories_describe_app_storage_and_existing_export_without_exposing_container_paths()
-> Result<(), String> {
    for os in [Os::Ios, Os::Android] {
        let mut config = EngineConfig::new(Root::of::<Directories>());
        config.environment.os = os;
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("text:App private directory")?;
        app.wait_for("text:Downloads stay in the app. Export files from Tasks.")?;
        app.wait_for("text:Cache is managed by the app. Use the cleanup controls below.")?;
        assert!(!app.screen().shows(PRIVATE_PATH));
        assert_eq!(app.screen().count("role:textbox")?, 0);
    }
    Ok(())
}
