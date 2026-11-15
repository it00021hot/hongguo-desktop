use nsh_core::{prelude::*, storage::Storage};

const POPUP_KEY: &str = "hongguo.followPopup";

#[derive(Clone, Copy)]
pub(crate) struct NoticePreferences {
    pub(crate) enabled: Signal<bool>,
    pub(super) active: Signal<bool>,
}

impl NoticePreferences {
    pub(crate) fn new() -> Self {
        Self {
            enabled: signal(Storage::get_item(POPUP_KEY).as_deref() == Some("1")),
            active: signal(false),
        }
    }

    pub(crate) fn set(self, enabled: bool) -> Result<(), String> {
        Storage::set_item(POPUP_KEY, if enabled { "1" } else { "0" })
            .map_err(|error| error.to_string())?;
        self.enabled.set(enabled);
        Ok(())
    }
}
