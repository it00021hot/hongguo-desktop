use crate::i18n::tr;
use nsh_core::{prelude::*, storage::Storage};

const COLLAPSED_KEY: &str = "guava.browseFiltersCollapsed";

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
