use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Settings() -> impl IntoElement {
    set_tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).expect("test theme"));
    let generation = signal(0);
    view()
        .child(
            crate::ui::button("Reopen", crate::ui::ButtonVariant::Quiet)
                .on_press(move || generation.update(|generation| *generation += 1)),
        )
        .child(each(
            move || vec![generation.get()],
            |generation| *generation,
            move |generation, _| {
                let state = Controller::new(preferences());
                view()
                    .child(text(move || format!("owner:{}", generation.get())))
                    .child(crate::ui::enhancement_settings(
                        state.settings,
                        state.phase,
                        state.message,
                    ))
            },
        ))
}

#[test]
fn independent_toggles_default_off_and_persist_across_player_owner_replacement()
-> Result<(), String> {
    let mut config =
        EngineConfig::new(Root::of::<Settings>().tokens(
            nsh_base::tokens_of(Some(include_str!("../../theme.json"))).expect("test theme"),
        ));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").expect("test locale");
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("role:button[Anime4K 2×]")?;
    assert!(!app.screen().selected("role:button[Anime4K 2×]")?);
    assert!(!app.screen().selected("role:button[锐化]")?);
    assert!(!app.screen().matches("role:slider[锐化强度]")?);
    app.press("role:button[锐化]")?;
    app.wait_for("text:50%")?;
    assert!(!app.screen().selected("role:button[Anime4K 2×]")?);
    app.press("role:button[Anime4K 2×]")?;
    app.press("Reopen")?;
    app.wait_for("text:owner:1")?;
    assert!(app.screen().selected("role:button[Anime4K 2×]")?);
    assert!(app.screen().selected("role:button[锐化]")?);
    app.press("role:button[锐化]")?;
    assert!(app.screen().selected("role:button[Anime4K 2×]")?);
    assert!(!app.screen().matches("role:slider[锐化强度]")?);
    app.press("role:button[Anime4K 2×]")?;
    app.press("Reopen")?;
    app.wait_for("text:owner:2")?;
    assert!(!app.screen().selected("role:button[Anime4K 2×]")?);
    assert!(!app.screen().selected("role:button[锐化]")?);
    Ok(())
}
