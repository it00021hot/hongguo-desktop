use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Reminders() -> impl IntoElement {
    view()
        .flex_1()
        .child(ui::stack().child(preview(Navigation::new(), Kind::Follows)))
}

#[test]
fn follow_reminders_are_a_reachable_working_tab() -> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<Reminders>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
    );
    config.environment.os = nsh_core::platform::Os::MacOs;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("role:button[Updates]")?;
    assert!(app.screen().enabled("role:button[Updates]")?);
    app.press("role:button[Updates]")?;
    app.wait_for("Check now")?;
    app.wait_for("Popup alerts")?;
    app.wait_for("No alerts")?;
    app.press("Check now")?;
    app.wait_for("3 new alerts")?;
    app.wait_for("id:follow-notices-unread >> text:2")?;
    app.wait_for("Now live")?;
    app.wait_for("role:button[归途有风 · 追的剧更新了，新增 2 集（共 24 集） · Update · Unread]")?;
    for _ in 0..6 {
        app.press_key("tab")?;
    }
    app.press_key("enter")?;
    app.wait_for("id:follow-notices-unread >> text:1")?;
    app.wait_for("role:button[归途有风 · 追的剧更新了，新增 2 集（共 24 集） · Update · Read]")?;
    app.press("Clear")?;
    app.wait_for("Cleared")?;
    app.wait_for("No alerts")?;
    assert!(!app.screen().matches("id:follow-notices-unread")?);
    Ok(())
}
