use crate::i18n::tr;
use nsh_core::{prelude::*, storage::Storage};

const COLLAPSED_KEY: &str = "hongguo.browseFiltersCollapsed";

pub(super) fn expanded() -> Signal<bool> {
    signal(
        Storage::get_item(COLLAPSED_KEY)
            .as_deref()
            .map_or_else(is_desktop, |value| value != "1"),
    )
}

pub(super) fn toggle(expanded: Signal<bool>) {
    let previous = expanded.get_untracked();
    let next = !previous;
    expanded.set(next);
    if let Err(error) = Storage::set_item(COLLAPSED_KEY, if next { "0" } else { "1" }) {
        expanded.set(previous);
        let description = tr("settings.saveFailed").replace("{message}", &error.to_string());
        spawn(async move {
            if let Err(error) = nsh_base::alert_dialog()
                .title(tr("settings.save"))
                .description(description)
                .cancel("close", tr("common.close"))
                .show()
                .await
            {
                log::error!("[NSH browse] could not present save failure: {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{ButtonVariant, button};
    use nsh_engine::{EngineConfig, FileStorage, Root};
    use nsh_testing::Headless;

    #[component]
    fn Preference() -> impl IntoElement {
        let expanded = expanded();
        view()
            .child(text(move || format!("expanded:{}", expanded.get())))
            .child(
                button("Toggle filters", ButtonVariant::Quiet).on_press(move || toggle(expanded)),
            )
    }

    fn start(directory: &std::path::Path) -> Result<Headless, String> {
        start_on(directory, nsh_core::platform::Os::MacOs)
    }

    fn start_on(
        directory: &std::path::Path,
        os: nsh_core::platform::Os,
    ) -> Result<Headless, String> {
        let mut config = EngineConfig::new(Root::of::<Preference>());
        config.environment.os = os;
        config.storage = Some(FileStorage::open(directory));
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("Toggle filters")?;
        Ok(app)
    }

    #[test]
    fn fresh_mobile_filters_start_collapsed_and_keep_the_users_explicit_choice()
    -> Result<(), String> {
        for os in [nsh_core::platform::Os::Ios, nsh_core::platform::Os::Android] {
            let directory = std::env::temp_dir().join(format!(
                "hongguo-browse-mobile-preference-{}",
                uuid::Uuid::new_v4()
            ));
            {
                let mut app = start_on(&directory, os)?;
                app.wait_for("text:expanded:false")?;
                app.press("Toggle filters")?;
                app.wait_for("text:expanded:true")?;
            }
            {
                let mut app = start_on(&directory, os)?;
                app.wait_for("text:expanded:true")?;
                app.press("Toggle filters")?;
                app.wait_for("text:expanded:false")?;
            }
            {
                let mut app = start_on(&directory, os)?;
                app.wait_for("text:expanded:false")?;
            }
            std::fs::remove_dir_all(directory).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    #[test]
    fn filter_expansion_survives_a_new_engine_and_can_be_restored() -> Result<(), String> {
        let directory = std::env::temp_dir().join(format!(
            "hongguo-browse-preference-{}",
            uuid::Uuid::new_v4()
        ));
        {
            let mut app = start(&directory)?;
            app.wait_for("text:expanded:true")?;
            app.press("Toggle filters")?;
            app.wait_for("text:expanded:false")?;
        }
        {
            let mut app = start(&directory)?;
            app.wait_for("text:expanded:false")?;
            app.press("Toggle filters")?;
            app.wait_for("text:expanded:true")?;
        }
        {
            let mut app = start(&directory)?;
            app.wait_for("text:expanded:true")?;
        }
        std::fs::remove_dir_all(directory).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn exhausted_storage_keeps_the_previous_filter_expansion_and_reports_failure()
    -> Result<(), String> {
        use nsh_core::storage::{QUOTA, StorageBackend, StorageChange};
        use std::{collections::BTreeMap, sync::Arc, time::Duration};

        struct Full;
        impl StorageBackend for Full {
            fn load(&self) -> Result<BTreeMap<String, String>, String> {
                Ok(BTreeMap::from([(
                    "filler".into(),
                    "x".repeat(QUOTA - "filler".len()),
                )]))
            }

            fn write(&self, _: StorageChange) {
                panic!("a rejected preference must not reach persistent storage");
            }
        }

        for (os, expanded) in [
            (nsh_core::platform::Os::MacOs, true),
            (nsh_core::platform::Os::Ios, false),
            (nsh_core::platform::Os::Android, false),
        ] {
            let mut config = EngineConfig::new(Root::of::<Preference>());
            config.environment.os = os;
            config.storage = Some(Arc::new(Full));
            let mut app = Headless::start(config).map_err(|error| error.to_string())?;
            app.wait_for(&format!("text:expanded:{expanded}"))?;
            app.press("Toggle filters")?;
            app.wait(Duration::from_millis(32))?;
            assert!(app.screen().matches(&format!("text:expanded:{expanded}"))?);
            assert!(
                !app.screen()
                    .matches(&format!("text:expanded:{}", !expanded))?
            );
            assert_eq!(app.screen().dialogs().len(), 1);
        }
        Ok(())
    }
}
