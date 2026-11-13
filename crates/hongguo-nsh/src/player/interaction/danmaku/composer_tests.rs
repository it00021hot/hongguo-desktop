use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn AnonymousComposer() -> impl IntoElement {
    let _notifications = crate::notifications::install();
    let loaded = resource(
        || Some(()),
        |_, _| {
            bridge::request(crate::bootstrap::Session::for_test(
                Default::default(),
                Vec::new(),
            ))
        },
    );
    when(
        move || loaded.get().is_some(),
        move || {
            let session = SessionHandle::new(loaded.get().unwrap());
            let handle = Handle {
                enabled: signal(true),
                timeline: signal(Timeline::default()),
                position: signal((0.0, 120.0)),
                motion: Motion::new(
                    computed(|| true),
                    signal(true),
                    computed(|| false),
                    signal(false),
                    signal(None),
                ),
                preferences: session.danmaku_preferences(),
                error: signal(SharedString::default()),
                retry: signal(0),
            };
            view()
                .child(handle.controls(session, computed(element_ref)))
                .child(text(move || handle.notice()))
        },
    )
}

#[test]
fn anonymous_danmaku_can_be_edited_and_submission_explains_login_without_clearing_it()
-> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<AnonymousComposer>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(500.0), px(200.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    let field = "label:发条友善的弹幕吧";
    app.wait_for(field)?;
    app.type_text(field, "测试弹幕abc123")?;
    app.wait_for("text:测试弹幕abc123")?;
    assert!(app.screen().enabled("role:button[发送]")?);
    app.press("role:button[发送]")?;
    app.wait_for("互动需要先登录（设置页 → 账号）")?;
    app.wait_for("text:测试弹幕abc123")?;
    Ok(())
}
