use nsh_core::{element::ImageSrc, prelude::*};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn BingeHost() -> impl IntoElement {
    super::fixture(ImageSrc::from(crate::emoji::SYSTEM[0].uri), || {})
}

#[test]
fn locking_binge_hides_only_the_entry_and_more_keeps_one_focusable_episode_sheet()
-> Result<(), String> {
    check(nsh_core::platform::Os::MacOs, true)
}

#[test]
fn ios_binge_keeps_the_more_episode_sheet_available_after_the_entry_is_hidden() -> Result<(), String>
{
    check(nsh_core::platform::Os::Ios, false)
}

fn check(os: nsh_core::platform::Os, keyboard: bool) -> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<BingeHost>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(480.0), px(840.0));
    config.environment.os = os;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    let entry = "role:button[观看完整短剧 · 全80集  ›]";
    app.wait_for(entry)?;
    app.press(entry)?;
    assert!(!app.screen().matches(entry)?);
    assert!(!app.screen().matches("id:player-episode-panel")?);
    app.press("role:button[倍速]")?;
    app.wait_for("id:mobile-player-more")
        .map_err(|error| format!("initial More open: {error}"))?;
    app.press("role:button[选集 · 全80集]")?;
    app.wait_for("id:player-episode-panel")?;
    assert_eq!(app.screen().count("id:player-episode-panel")?, 1);
    assert!(!app.screen().matches("id:mobile-player-more")?);
    app.press_key("escape")?;
    app.wait_for_gone("id:player-episode-panel", std::time::Duration::from_secs(1))?;
    if keyboard {
        app.press_key("enter")?;
    } else {
        app.press("role:button[倍速]")?;
    }
    app.wait_for("id:mobile-player-more")
        .map_err(|error| format!("focus return after Episodes close: {error}"))?;
    Ok(())
}
