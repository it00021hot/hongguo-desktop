use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Reopened() -> impl IntoElement {
    let generation = signal(0);
    view()
        .child(
            crate::ui::button("Reopen preferences", crate::ui::ButtonVariant::Quiet)
                .on_press(move || generation.update(|value| *value += 1)),
        )
        .child(each(
            move || vec![generation.get()],
            |generation| *generation,
            move |generation, _| {
                let preferences = DanmakuPreferences::new();
                let state = preferences.display;
                view()
                    .child(
                        crate::ui::button("Adjust preferences", crate::ui::ButtonVariant::Quiet)
                            .on_press(move || {
                                batch(|| {
                                    preferences.enabled.set(false);
                                    state.opacity.set(0.8);
                                    state.font_scale.set(1.5);
                                    state.density.set(0.25);
                                    state.area.set(0.75);
                                })
                            }),
                    )
                    .child(text(move || {
                        format!(
                            "settings{} {} {} {} {} {}",
                            generation.get(),
                            preferences.enabled.get(),
                            state.opacity.get(),
                            state.font_scale.get(),
                            state.density.get(),
                            state.area.get()
                        )
                    }))
            },
        ))
}

#[test]
fn reopening_reads_all_four_saved_parameters_and_the_independent_switch() -> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<Reopened>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("settings0 true 0.5 0.5 0.5 0.25")?;
    app.press("Adjust preferences")?;
    app.wait_for("settings0 false 0.8 1.5 0.25 0.75")?;
    app.press("Reopen preferences")?;
    app.wait_for("settings1 false 0.8 1.5 0.25 0.75")?;
    Ok(())
}

#[component]
fn Malformed() -> impl IntoElement {
    for (key, value) in [
        (DanmakuField::Opacity.key(), "NaN"),
        (DanmakuField::FontScale.key(), "999"),
        (DanmakuField::Density.key(), "-1"),
        (DanmakuField::Area.key(), "garbled"),
    ] {
        Storage::set_item(key, value).expect("test storage");
    }
    let state = DanmakuPreferences::new().display;
    text(move || {
        format!(
            "validated {} {} {} {}",
            state.opacity.get(),
            state.font_scale.get(),
            state.density.get(),
            state.area.get()
        )
    })
}

#[test]
fn malformed_and_out_of_range_storage_never_reach_the_overlay() -> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<Malformed>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("validated 0.5 2 0 0.25")?;
    Ok(())
}
