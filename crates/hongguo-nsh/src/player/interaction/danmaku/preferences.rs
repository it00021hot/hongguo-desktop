use crate::{
    i18n::tr,
    ui::{DanmakuField, DanmakuSettingsState},
};
use nsh_core::{prelude::*, storage::Storage};

const ENABLED_KEY: &str = "hongguo.danmaku";

#[derive(Clone, Copy)]
pub(crate) struct DanmakuPreferences {
    pub(super) enabled: Signal<bool>,
    pub(super) display: DanmakuSettingsState,
    pub(super) error: Signal<SharedString>,
}

impl DanmakuPreferences {
    pub(crate) fn new() -> Self {
        let value = Self {
            enabled: signal(Storage::get_item(ENABLED_KEY).as_deref() != Some("0")),
            display: DanmakuSettingsState::new(),
            error: signal(SharedString::default()),
        };
        for field in DanmakuField::ALL {
            let initial = read(field);
            let state = field.value(value.display);
            state.set(initial);
            let previous = stored(initial);
            effect(move || {
                let current = state.get();
                if previous.replace(current) != current {
                    value.persist(field.key(), current.to_string());
                }
            });
        }
        let previous = stored(value.enabled.get_untracked());
        effect(move || {
            let enabled = value.enabled.get();
            if previous.replace(enabled) != enabled {
                value.persist(ENABLED_KEY, if enabled { "1" } else { "0" }.into());
            }
        });
        value
    }
    fn persist(self, key: &str, value: String) {
        if let Err(error) = Storage::set_item(key, value) {
            log::error!("[NSH danmaku preferences] failed to save {key}: {error}");
            self.error.set(
                tr("settings.saveFailed")
                    .replace("{message}", &error.to_string())
                    .into(),
            );
        } else {
            self.error.set(SharedString::default());
        }
    }
}

fn read(field: DanmakuField) -> f32 {
    let (min, max) = field.range();
    Storage::get_item(field.key())
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(min, max))
        .unwrap_or(field.default_value())
}

#[cfg(test)]
#[path = "preferences_tests.rs"]
mod tests;
