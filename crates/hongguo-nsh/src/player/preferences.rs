use nsh_core::prelude::*;
use nsh_core::storage::Storage;

const RATE_KEY: &str = "hongguo.playbackRate";
const VOLUME_KEY: &str = "hongguo.volume";
const MUTED_KEY: &str = "hongguo.muted";
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

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    #[component]
    fn ReplacedVideo() -> impl IntoElement {
        let preferences = PreferenceSignals::new();
        let generation = signal(0_u32);
        view()
            .child(
                crate::ui::button("Adjust controls", crate::ui::ButtonVariant::Quiet).on_press(
                    move || {
                        batch(|| {
                            preferences.rate.set(1.5);
                            preferences.volume.set(0.25);
                            preferences.muted.set(true);
                        });
                    },
                ),
            )
            .child(
                crate::ui::button("Replace video", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || generation.update(|generation| *generation += 1)),
            )
            .child(each(
                move || vec![generation.get()],
                |generation: &u32| *generation,
                move |generation, _| {
                    text(move || {
                        format!(
                            "video{}:{}/{}/{}",
                            generation.get(),
                            preferences.rate.get(),
                            preferences.volume.get(),
                            preferences.muted.get()
                        )
                    })
                },
            ))
    }

    #[test]
    fn session_controls_survive_replacing_the_entire_video_owner() -> Result<(), String> {
        let mut app = Headless::start(EngineConfig::new(Root::of::<ReplacedVideo>()))
            .map_err(|error| error.to_string())?;
        app.wait_for("Adjust controls")?;
        app.press("Adjust controls")?;
        app.wait_for("text:video0:1.5/0.25/true")?;
        app.press("Replace video")?;
        app.wait_for("text:video1:1.5/0.25/true")?;
        Ok(())
    }

    #[component]
    fn ReopenedSession() -> impl IntoElement {
        let generation = signal(0);
        view()
            .flex_1()
            .child(
                crate::ui::button("Reopen session", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || generation.update(|generation| *generation += 1)),
            )
            .child(each(
                move || vec![generation.get()],
                |generation| *generation,
                move |generation, _| {
                    let preferences = PreferenceSignals::new();
                    view()
                        .child(
                            crate::ui::button(
                                "Change preferences",
                                crate::ui::ButtonVariant::Quiet,
                            )
                            .on_press(move || {
                                batch(|| {
                                    preferences.rate.set(1.75);
                                    preferences.volume.set(0.3);
                                    preferences.muted.set(true);
                                });
                            }),
                        )
                        .child(text(move || {
                            format!(
                                "session{}:{}/{}/{}",
                                generation.get(),
                                preferences.rate.get(),
                                preferences.volume.get(),
                                preferences.muted.get()
                            )
                        }))
                },
            ))
    }

    #[test]
    fn a_new_session_reads_the_saved_rate_volume_and_independent_mute() -> Result<(), String> {
        let mut app = Headless::start(EngineConfig::new(Root::of::<ReopenedSession>()))
            .map_err(|error| error.to_string())?;
        app.wait_for("text:session0:1/1/false")?;
        app.press("Change preferences")?;
        app.wait_for("text:session0:1.75/0.3/true")?;
        app.press("Reopen session")?;
        app.wait_for("text:session1:1.75/0.3/true")?;
        Ok(())
    }
}
