use super::*;
use crate::ui;
use nsh_core::desktop::{DesktopCommand, WindowMode, WindowRequest};
use nsh_engine::{Clock, EngineConfig, Environment, Root, Size};
use nsh_testing::Headless;
use std::{
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[component]
fn FullscreenController() -> impl IntoElement {
    let bridge = install(computed(|| false));
    let modes = WindowModes::current().unwrap();
    view()
        .child(bridge)
        .child(
            ui::button("Toggle native fullscreen", ui::ButtonVariant::Quiet).on_press(move || {
                modes.receive(&nsh_core::element::NativeEvent {
                    name: "toggleFullscreen".into(),
                    data: serde_json::Value::Null,
                });
            }),
        )
        .child(
            ui::button("Exit native fullscreen", ui::ButtonVariant::Quiet).on_press(move || {
                modes.receive(&nsh_core::element::NativeEvent {
                    name: "exitFullscreen".into(),
                    data: serde_json::Value::Null,
                });
            }),
        )
        .child(text(move || {
            format!(
                "native-mode:{:?}",
                nsh_core::element::desktop::current_window()
                    .state()
                    .map(|state| state.mode)
            )
        }))
}

fn start() -> Headless {
    let mut app = Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::Windows,
            ..Default::default()
        },
        viewport: Size::new(px(960.0), px(720.0)),
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            Root::of::<FullscreenController>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
        )
    })
    .unwrap();
    app.wait_for("text:native-mode:Some(Normal)").unwrap();
    app
}

fn report_mode(app: &mut Headless, mode: WindowMode) {
    app.set_environment("native window mode changed", move |environment| {
        environment.window_state.as_mut().unwrap().mode = mode;
    })
    .unwrap();
    app.wait_for(&format!("text:native-mode:Some({mode:?})"))
        .unwrap();
}

#[test]
fn fullscreen_exit_restores_the_last_windowed_mode_and_repeated_exit_sends_no_command() {
    for previous in [WindowMode::Maximized, WindowMode::Normal] {
        for entered_by_os in [true, false] {
            let mut app = start();
            let commands = Arc::new(Mutex::new(Vec::new()));
            let observed = commands.clone();
            app.observe(move |_, batch| {
                for item in &batch.items {
                    if let nsh_core::MountItem::Command {
                        command: nsh_core::Command::Desktop(command),
                        ..
                    } = item
                        && let DesktopCommand::Request(WindowRequest::SetMode(mode)) =
                            command.as_ref()
                    {
                        observed.lock().unwrap().push(*mode);
                    }
                }
            });
            report_mode(&mut app, previous);
            if entered_by_os {
                report_mode(&mut app, WindowMode::FullScreen);
            } else {
                app.press("Toggle native fullscreen").unwrap();
                app.wait_for("text:native-mode:Some(FullScreen)").unwrap();
                assert_eq!(*commands.lock().unwrap(), [WindowMode::FullScreen]);
            }

            app.press(if entered_by_os {
                "Exit native fullscreen"
            } else {
                "Toggle native fullscreen"
            })
            .unwrap();
            let expected = if entered_by_os {
                vec![previous]
            } else {
                vec![WindowMode::FullScreen, previous]
            };
            assert_eq!(*commands.lock().unwrap(), expected);
            if entered_by_os {
                // set_environment reports OS changes without mutating Headless's window backend.
                report_mode(&mut app, previous);
            }
            app.wait_for(&format!("text:native-mode:Some({previous:?})"))
                .unwrap();
            assert_eq!(app.window_state("main").unwrap().unwrap().mode, previous);

            app.press("Exit native fullscreen").unwrap();
            assert_eq!(*commands.lock().unwrap(), expected);
            assert_eq!(app.window_state("main").unwrap().unwrap().mode, previous);
        }
    }
}
