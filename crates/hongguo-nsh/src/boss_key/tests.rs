use super::*;
use crate::{
    shortcuts::Shortcuts,
    ui::{self, ButtonVariant},
};
use nsh_core::element::NativeEvent;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use serde_json::json;

const REGISTERED: &str = "CommandOrControl+Alt+K";

#[component]
fn Controller() -> impl IntoElement {
    let shortcuts = Shortcuts::provide_preview();
    let native = install(String::new());
    let state = use_context::<BossKey>().expect("installed controller");
    let result = signal(SharedString::from("idle"));
    let capture = signal(String::new());
    let request = move |spec: &'static str| {
        result.set("pending".into());
        spawn(async move {
            result.set(match apply(spec.to_owned()).await {
                Ok(()) => "registered".into(),
                Err(error) => error,
            });
        });
    };
    view().flex_1().child(native)
        .child(ui::button("Register", ButtonVariant::Quiet).on_press(move || request(REGISTERED)))
        .child(ui::button("Change", ButtonVariant::Quiet).on_press(move || request("invalid")))
        .child(ui::button("Success", ButtonVariant::Quiet).on_press(move || state.receive(&NativeEvent { name: "registered".into(), data: json!({"version":state.request.get_untracked().0,"error":null}) })))
        .child(ui::button("Failure", ButtonVariant::Quiet).on_press(move || state.receive(&NativeEvent { name: "registered".into(), data: json!({"version":state.request.get_untracked().0,"error":"native denied"}) })))
        .child(ui::button("Old response", ButtonVariant::Quiet).on_press(move || state.receive(&NativeEvent { name: "registered".into(), data: json!({"version":0,"error":null}) })))
        .child(ui::button("Capture boss", ButtonVariant::Quiet).on_press(move || shortcuts.capture_boss(move |spec| capture.set(spec))))
        .child(ui::button("Native press", ButtonVariant::Quiet).on_press(move || state.receive(&NativeEvent { name: "pressed".into(), data: json!(null) })))
        .child(text(result))
        .child(text(move || format!("active:{}", state.registered.get())))
        .child(text(move || format!("hidden:{}", state.hidden.get())))
        .child(text(move || format!("captured:{}", capture.get())))
}

fn start() -> Headless {
    let mut config = EngineConfig::new(Root::of::<Controller>());
    config.environment.os = nsh_core::platform::Os::MacOs;
    Headless::start(config).expect("boss key controller")
}

#[test]
fn stale_and_failed_native_acknowledgements_cannot_replace_the_registered_binding()
-> Result<(), String> {
    let mut app = start();
    app.wait_for("Register")?;
    app.press("Register")?;
    app.wait_for("pending")?;
    app.press("Old response")?;
    app.wait_for("pending")?;
    app.press("Success")?;
    app.wait_for("registered")?;
    app.wait_for(&format!("text:active:{REGISTERED}"))?;
    app.press("Change")?;
    app.wait_for("pending")?;
    app.press("Failure")?;
    app.wait_for("native denied")?;
    app.wait_for(&format!("text:active:{REGISTERED}"))?;
    Ok(())
}

#[test]
fn recapturing_the_registered_os_chord_uses_the_successful_spec_without_hiding_the_window()
-> Result<(), String> {
    let mut app = start();
    app.wait_for("Register")?;
    app.press("Register")?;
    app.wait_for("pending")?;
    app.press("Success")?;
    app.wait_for("registered")?;
    app.press("Capture boss")?;
    app.press("Native press")?;
    app.wait_for(&format!("text:captured:{REGISTERED}"))?;
    app.wait_for("text:hidden:false")?;
    app.press("Native press")?;
    app.wait_for("text:hidden:true")?;
    app.press("Native press")?;
    app.wait_for("text:hidden:false")?;
    Ok(())
}
