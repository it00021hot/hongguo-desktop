use super::*;
use crate::ui::{ButtonVariant, button};
use nsh_core::platform::Os;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn ExportFlow() -> impl IntoElement {
    let host = install();
    let files = FileAccess::current().unwrap();
    let result = signal(String::from("idle"));
    view()
        .child(host)
        .child(text(move || format!("result:{}", result.get())))
        .child(text(move || {
            files.request.with(|request| {
                format!(
                    "serial:{}",
                    request.as_ref().map_or(0, |request| request.serial)
                )
            })
        }))
        .child(button("Export", ButtonVariant::Quiet).on_press(move || {
            result.set("pending".into());
            spawn(async move {
                let outcome = files.export("/fixture/episode.mp4".into()).await;
                result.set(match outcome {
                    Ok(true) => "completed".into(),
                    Ok(false) => "cancelled".into(),
                    Err(error) => format!("failed:{error}"),
                });
            });
        }))
        .children(
            [
                ("Complete", false, false, None),
                ("Cancel", false, true, None),
                ("Fail", false, false, Some("Disk full")),
                ("Stale completion", true, false, None),
            ]
            .map(move |(label, stale, is_cancelled, error)| {
                button(label, ButtonVariant::Quiet).on_press(move || {
                    let serial = files
                        .request
                        .with(|request| request.as_ref().unwrap().serial);
                    files.receive(ExportResponse {
                        serial: if stale {
                            serial.wrapping_sub(1)
                        } else {
                            serial
                        },
                        is_cancelled,
                        error: error.map(str::to_owned),
                    });
                })
            }),
        )
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(Root::of::<ExportFlow>());
    config.environment.os = Os::Android;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("text:result:idle")?;
    Ok(app)
}

#[test]
fn export_cancel_failure_and_retry_follow_the_matching_native_completion() -> Result<(), String> {
    let mut app = start()?;
    app.press("Export")?;
    app.wait_for("text:serial:1")?;
    app.press("Stale completion")?;
    app.wait_for("text:result:pending")?;
    app.press("Cancel")?;
    app.wait_for("text:result:cancelled")?;
    app.press("Export")?;
    app.wait_for("text:serial:2")?;
    app.press("Fail")?;
    app.wait_for("text:result:failed:Disk full")?;
    app.press("Export")?;
    app.wait_for("text:serial:3")?;
    app.press("Complete")?;
    app.wait_for("text:result:completed")?;
    Ok(())
}

#[test]
fn a_second_export_cannot_replace_an_inflight_system_picker() -> Result<(), String> {
    let mut app = start()?;
    app.press("Export")?;
    app.wait_for("text:serial:1")?;
    app.press("Export")?;
    app.wait_for("text:result:failed:A file export is already in progress")?;
    app.press("Complete")?;
    app.wait_for("text:result:completed")?;
    app.wait_for("text:serial:1")?;
    Ok(())
}
