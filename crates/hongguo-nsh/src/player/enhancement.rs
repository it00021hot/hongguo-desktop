use hongguo_nsh_video_api::{Enhancement, EnhancementPhase};
use nsh_core::{prelude::*, storage::Storage};
#[cfg(test)]
#[path = "enhancement_tests.rs"]
mod tests;

const KEY: &str = "hongguo.enhance";
const MAX_STRENGTH: u8 = 100;

#[derive(Clone, Copy)]
pub(crate) struct Controller {
    pub open: Signal<bool>,
    pub settings: Signal<Enhancement>,
    pub phase: Signal<EnhancementPhase>,
    pub message: Signal<SharedString>,
}

impl Controller {
    pub fn new(settings: Signal<Enhancement>) -> Self {
        Self {
            open: signal(false),
            settings,
            phase: signal(EnhancementPhase::Idle),
            message: signal(SharedString::default()),
        }
    }

    pub fn current() -> Option<Self> {
        use_context()
    }
}

pub(super) fn preferences() -> Signal<Enhancement> {
    let initial = match Storage::get_item(KEY) {
        Some(raw) => match serde_json::from_str::<Enhancement>(&raw) {
            Ok(settings) if settings.strength <= MAX_STRENGTH => settings,
            _ => {
                log::warn!("[NSH enhancement] invalid stored settings; using original picture");
                Enhancement::default()
            }
        },
        None => Enhancement::default(),
    };
    let settings = signal(initial);
    let previous = stored(initial);
    effect(move || {
        let value = settings.get();
        if previous.replace(value) == value {
            return;
        }
        let result = serde_json::to_string(&value)
            .map_err(|error| error.to_string())
            .and_then(|json| Storage::set_item(KEY, json).map_err(|error| error.to_string()));
        if let Err(error) = result {
            log::error!("[NSH enhancement] could not save preferences: {error}");
        }
    });
    settings
}
