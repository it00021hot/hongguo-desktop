use super::{Mode, geometry::Rect};
use nsh_core::{
    desktop::{WindowMode, WindowPlacement},
    element::desktop,
    storage::Storage,
};

pub(crate) fn show_main_window() {
    let window = desktop::main_window();
    let placement = load(Mode::Normal);
    if let Some(placement) = placement {
        window.set_placement(WindowPlacement::new(WindowMode::Normal, placement.bounds));
    }
    window.show();
    if placement.is_some_and(|placement| placement.mode == WindowMode::Maximized) {
        window.set_mode(WindowMode::Maximized);
    }
}

pub(super) fn load(mode: Mode) -> Option<WindowPlacement> {
    let key = storage_key(mode);
    let stored = Storage::get_item(key)?;
    match serde_json::from_str::<WindowPlacement>(&stored) {
        Ok(placement) if Rect::from(placement.bounds).is_valid() => Some(placement),
        Ok(_) => {
            log::warn!("[NSH window] invalid placement in {key}");
            None
        }
        Err(error) => {
            log::warn!("[NSH window] cannot read {key}: {error}");
            None
        }
    }
}

pub(super) fn save(mode: Mode, placement: WindowPlacement) {
    let key = storage_key(mode);
    let result = serde_json::to_string(&placement).map_err(|error| error.to_string());
    let result = result.and_then(|value| {
        if Storage::get_item(key).as_deref() == Some(value.as_str()) {
            return Ok(());
        }
        Storage::set_item(key, value).map_err(|error| error.to_string())
    });
    if let Err(error) = result {
        log::error!("[NSH window] cannot save {key}: {error}");
    }
}

fn storage_key(mode: Mode) -> &'static str {
    match mode {
        Mode::Normal => "guava.windowPlacement.normal",
        Mode::Mini => "guava.windowPlacement.mini",
        Mode::Portrait => "guava.windowPlacement.portrait",
    }
}
