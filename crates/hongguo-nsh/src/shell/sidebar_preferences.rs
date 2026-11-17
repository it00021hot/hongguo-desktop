use crate::i18n::tr;
use nsh_core::{prelude::*, storage::Storage};

const COLLAPSED_KEY: &str = "guava.sidebarCollapsed";

pub(super) fn collapsed() -> Signal<bool> {
    signal(Storage::get_item(COLLAPSED_KEY).as_deref() != Some("0"))
}

pub(super) fn toggle(collapsed: Signal<bool>) {
    let next = !collapsed.get_untracked();
    match Storage::set_item(COLLAPSED_KEY, if next { "1" } else { "0" }) {
        Ok(()) => collapsed.set(next),
        Err(error) => {
            let description = tr("settings.saveFailed").replace("{message}", &error.to_string());
            spawn(async move {
                if let Err(error) = nsh_base::alert_dialog()
                    .title(tr("settings.save"))
                    .description(description)
                    .cancel("close", tr("common.close"))
                    .show()
                    .await
                {
                    log::error!("[NSH sidebar] could not present save failure: {error}");
                }
            });
        }
    }
}
