use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn LoadingInformation() -> impl IntoElement {
    let information = signal(super::Info::default());
    view().child(super::info(information, || {})).child(
        crate::ui::button("Load", crate::ui::ButtonVariant::Quiet).on_press(move || {
            information.set(super::Info {
                title: "已加载剧目".into(),
                episode: 1,
                total: 80,
                ..Default::default()
            });
        }),
    )
}

#[test]
fn loading_information_never_exposes_a_zero_episode_or_an_empty_title() -> Result<(), String> {
    for (os, title) in [
        (nsh_core::platform::Os::MacOs, "@已加载剧目"),
        (nsh_core::platform::Os::Ios, "已加载剧目"),
        (nsh_core::platform::Os::Android, "已加载剧目"),
    ] {
        let mut config = EngineConfig::new(Root::of::<LoadingInformation>().tokens(
            nsh_base::tokens_of(Some(include_str!("../../../theme.json")))?,
        ));
        config.environment.os = os;
        config.environment.locale =
            nsh_core::platform::Locale::parse("zh-CN").expect("valid test locale");
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("Load")?;
        assert!(!app.screen().matches("text:@")?);
        assert!(!app.screen().matches("text:第0集 · 全0集")?);
        app.press("Load")?;
        app.wait_for(&format!("text:{title}"))?;
        app.wait_for("text:第1集 · 全80集")?;
    }
    Ok(())
}
