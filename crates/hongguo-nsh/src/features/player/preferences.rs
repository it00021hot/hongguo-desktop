use nsh_core::prelude::*;
use nsh_core::storage::Storage;

const RATE_KEY: &str = "guava.playbackRate";
const VOLUME_KEY: &str = "guava.volume";
const MUTED_KEY: &str = "guava.muted";
pub(super) const MIN_RATE: f32 = 0.25;
pub(super) const MAX_RATE: f32 = 4.0;

/// The session owns these controls; replacing an embedded video never resets them.
#[derive(Clone, Copy)]
pub(crate) struct PreferenceSignals {
    pub(super) enhancement: Signal<hongguo_nsh_video_api::Enhancement>,
    pub(super) rate: Signal<f32>,
    pub(super) volume: Signal<f32>,
    pub(super) muted: Signal<bool>,
}

impl PreferenceSignals {
    pub(crate) fn new() -> Self {
        let rate = read_number(RATE_KEY, 1.0, MIN_RATE, MAX_RATE);
        let volume = read_number(VOLUME_KEY, 1.0, 0.0, 1.0);
        let muted = read_number(MUTED_KEY, 0.0, 0.0, 1.0) == 1.0;
        let state = Self {
            enhancement: super::enhancement::preferences(),
            rate: signal(rate),
            volume: signal(volume),
            muted: signal(muted),
        };
        persist(RATE_KEY, state.rate, rate);
        persist(VOLUME_KEY, state.volume, volume);
        let previous = stored(muted);
        effect(move || {
            let muted = state.muted.get();
            if previous.replace(muted) != muted {
                write(MUTED_KEY, if muted { "1" } else { "0" });
            }
        });
        state
    }
}

fn read_number(key: &str, default: f32, min: f32, max: f32) -> f32 {
    let Some(value) = Storage::get_item(key) else {
        return default;
    };
    match value.parse::<f32>() {
        Ok(value) if value.is_finite() && (min..=max).contains(&value) => value,
        _ => {
            log::warn!("[NSH playback preferences] invalid {key}; using default");
            default
        }
    }
}

fn persist(key: &'static str, value: Signal<f32>, initial: f32) {
    let previous = stored(initial);
    effect(move || {
        let value = value.get();
        if previous.replace(value) != value {
            write(key, value.to_string());
        }
    });
}

fn write(key: &str, value: impl Into<String>) {
    if let Err(error) = Storage::set_item(key, value) {
        log::error!("[NSH playback preferences] could not persist {key}: {error}");
    }
}
