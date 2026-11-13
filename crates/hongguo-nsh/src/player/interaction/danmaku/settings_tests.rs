use nsh_core::{element::ImageSrc, prelude::*};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Host() -> impl IntoElement {
    crate::player::stage::fixture(
        ImageSrc::svg_markup(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>",
        ),
        || {},
    )
}

#[test]
fn the_production_settings_button_opens_four_live_sliders_independently_of_other_advanced_controls()
-> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<Host>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(1280.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("role:button[弹幕设置]")?;
    assert!(
        !app.screen()
            .accessibility_state("role:button[弹幕设置]")?
            .disabled
    );
    assert!(
        app.screen()
            .accessibility_state("role:button[小屏播放]")?
            .disabled
    );
    app.press("role:button[弹幕设置]")?;
    for label in ["不透明度", "字体大小", "密度", "显示区域"] {
        app.wait_for(&format!("role:adjustable[{label}]"))?;
    }
    app.drag("role:adjustable[不透明度]", point(px(240.0), px(0.0)))?;
    app.wait_for("100%")?;
    app.press_key("escape")?;
    app.wait_for_gone(
        "role:adjustable[不透明度]",
        std::time::Duration::from_secs(1),
    )?;
    app.press("role:button[弹幕设置]")?;
    app.wait_for("100%")?;
    Ok(())
}
