use super::*;
use crate::{navigation::Navigation, ui};
use nsh_engine::{Clock, EngineConfig, Environment, Root, Size};
use nsh_testing::Headless;
use std::{
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[component]
fn Controller() -> impl IntoElement {
    let driver: PreviewDriver = provide_preview();
    let modes = WindowModes::current().unwrap();
    let nav = Navigation::new();
    let body = view()
        .flex_1()
        .child(
            view()
                .test_id("stable-video-owner")
                .on_press(|| {})
                .child(text("Video owner")),
        )
        .child(ui::button("Mini", ui::ButtonVariant::Quiet).on_press(move || modes.toggle_mini()))
        .child(
            ui::button("Portrait", ui::ButtonVariant::Quiet)
                .on_press(move || modes.toggle_portrait(0.36)),
        )
        .child(
            ui::button("Fullscreen", ui::ButtonVariant::Quiet)
                .on_press(move || modes.toggle_fullscreen()),
        )
        .child(
            ui::button("Exit window mode", ui::ButtonVariant::Quiet).on_press(move || modes.exit()),
        )
        .child(ui::button("Ack", ui::ButtonVariant::Quiet).on_press(move || driver.acknowledge()))
        .child(
            ui::button("Deny", ui::ButtonVariant::Quiet).on_press(move || driver.fail("OS denied")),
        )
        .child(
            ui::button("Old ack", ui::ButtonVariant::Quiet).on_press(move || {
                modes.receive(&nsh_core::element::NativeEvent {
                    name: "applied".into(),
                    data: serde_json::to_value(Response {
                        serial: 0,
                        state: State::initial(),
                        error: None,
                    })
                    .unwrap(),
                })
            }),
        )
        .child(text(move || {
            format!(
                "mode:{:?} pending:{} fill:{}",
                modes.state().mode,
                modes.pending(),
                modes.state().portrait_fill
            )
        }))
        .child(text(move || format!("error:{}", modes.error())))
        .child(text(move || {
            format!(
                "fullscreen:{} immersive:{}",
                modes.state().fullscreen,
                modes.immersive()
            )
        }))
        .child(text(move || {
            modes
                .request
                .get()
                .map_or_else(String::new, |request| match request.action {
                    Action::SetPortraitFill { aspect, .. } | Action::Refit { aspect } => {
                        format!("aspect:{aspect}")
                    }
                    _ => String::new(),
                })
        }));
    ui::shell(nav, view().test_id("fixture-toolbar-slot"), body)
}

fn start() -> Headless {
    let mut app = Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::MacOs,
            locale: nsh_core::platform::Locale::parse("zh-CN").unwrap(),
            ..Default::default()
        },
        viewport: Size::new(px(480.0), px(840.0)),
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            Root::of::<Controller>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
        )
    })
    .unwrap();
    app.wait_for("Video owner").unwrap();
    app
}

#[test]
fn preview_records_requests_and_requires_current_ack_before_changing_window_state() {
    let mut app = start();
    app.press("Mini").unwrap();
    app.wait_for("text:mode:Normal pending:true fill:false")
        .unwrap();
    app.press("Old ack").unwrap();
    app.wait_for("text:mode:Normal pending:true fill:false")
        .unwrap();
    app.press("Ack").unwrap();
    app.wait_for("text:mode:Mini pending:false fill:false")
        .unwrap();
    app.press("Portrait").unwrap();
    app.press("Deny").unwrap();
    app.wait_for("text:mode:Mini pending:false fill:false")
        .unwrap();
    app.wait_for("text:error:OS denied").unwrap();
    app.press("Ack").unwrap();
    app.wait_for("text:mode:Mini pending:false fill:false")
        .unwrap();
    app.press("Portrait").unwrap();
    app.press("Ack").unwrap();
    app.wait_for("text:mode:Portrait pending:false fill:false")
        .unwrap();
    app.press("Fullscreen").unwrap();
    app.wait_for("text:aspect:0.36").unwrap();
    app.press("Ack").unwrap();
    app.wait_for("text:mode:Portrait pending:false fill:true")
        .unwrap();
}

#[test]
fn standalone_fullscreen_is_immersive_and_exit_clears_it_after_confirmation() {
    let mut app = start();
    app.press("Fullscreen").unwrap();
    app.press("Ack").unwrap();
    app.wait_for("text:fullscreen:true immersive:true").unwrap();
    assert!(!app.screen().matches("id:app-titlebar").unwrap());
    app.press("Exit window mode").unwrap();
    app.wait_for("text:fullscreen:true immersive:true").unwrap();
    app.press("Ack").unwrap();
    app.wait_for("text:fullscreen:false immersive:false")
        .unwrap();
    assert!(app.screen().matches("id:app-titlebar").unwrap());
}

#[test]
fn mini_and_portrait_hide_only_chrome_while_preserving_the_same_content_owner_and_do_not_call_the_os()
 {
    let mut app = start();
    let owner = app.screen().press_target("id:stable-video-owner").unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let nsh_core::MountItem::Command {
                command: nsh_core::Command::Desktop(command),
                ..
            } = item
            {
                observed.lock().unwrap().push(format!("{command:?}"));
            }
        }
    });
    assert!(app.screen().matches("id:app-titlebar").unwrap());
    assert!(app.screen().matches("id:bottom-nav").unwrap());
    app.press("Mini").unwrap();
    app.press("Ack").unwrap();
    assert!(!app.screen().matches("id:app-titlebar").unwrap());
    assert!(!app.screen().matches("id:bottom-nav").unwrap());
    assert_eq!(
        app.screen().press_target("id:stable-video-owner").unwrap(),
        owner
    );
    app.press("Portrait").unwrap();
    app.press("Ack").unwrap();
    assert!(app.screen().matches("id:app-titlebar").unwrap());
    assert!(!app.screen().matches("id:bottom-nav").unwrap());
    app.press("Exit window mode").unwrap();
    app.press("Ack").unwrap();
    assert!(app.screen().matches("id:app-titlebar").unwrap());
    assert!(app.screen().matches("id:bottom-nav").unwrap());
    assert_eq!(
        app.screen().press_target("id:stable-video-owner").unwrap(),
        owner
    );
    assert!(
        calls.lock().unwrap().is_empty(),
        "preview must never resize, pin, fullscreen or hide its real host: {:?}",
        calls.lock().unwrap()
    );
}
