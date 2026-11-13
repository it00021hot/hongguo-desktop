use super::*;
use crate::ui::{self, ButtonVariant};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn NotificationHost() -> impl IntoElement {
    let host = install();
    let state = use_context::<Notifications>().expect("notification context");
    let outcome = signal(String::from("idle"));
    let first = signal(String::from("first idle"));
    let second = signal(String::from("second idle"));
    view()
        .child(host)
        .child(text(move || {
            format!("pending {}", state.requests.get().len())
        }))
        .child(text(move || outcome.get()))
        .child(text(move || first.get()))
        .child(text(move || second.get()))
        .child(
            ui::button("Send first", ButtonVariant::Quiet).on_press(move || {
                spawn(async move {
                    first.set(format!(
                        "first {:?}",
                        state.send("first", "Title & <tag>", "Body").await
                    ));
                });
            }),
        )
        .child(
            ui::button("Send second", ButtonVariant::Quiet).on_press(move || {
                spawn(async move {
                    second.set(format!(
                        "second {:?}",
                        state.send("second", "Title", "Body").await
                    ));
                });
            }),
        )
        .child(
            ui::button("Ack second", ButtonVariant::Quiet).on_press(move || {
                state.receive(Response {
                    serial: 2,
                    granted: true,
                    error: None,
                });
            }),
        )
        .child(
            ui::button("Reject first", ButtonVariant::Quiet).on_press(move || {
                state.receive(Response {
                    serial: 1,
                    granted: false,
                    error: Some("native refused".into()),
                });
            }),
        )
        .child(ui::button("Flood", ButtonVariant::Quiet).on_press(move || {
            for _ in 0..=MAX_PENDING {
                spawn(async move {
                    if let Err(error) = state.request_permission().await {
                        outcome.set(error);
                    }
                });
            }
        }))
        .child(
            ui::button("Enable", ButtonVariant::Quiet).on_press(move || {
                spawn(async move {
                    outcome.set(format!("permission {:?}", state.request_permission().await));
                });
            }),
        )
        .child(ui::button("Deny", ButtonVariant::Quiet).on_press(move || {
            state.receive(Response {
                serial: 1,
                granted: false,
                error: None,
            });
        }))
        .child(when(
            || true,
            move || {
                let other_host = install();
                let other = Notifications::current();
                view()
                    .child(other_host)
                    .child(text(move || {
                        format!("other pending {}", other.requests.get().len())
                    }))
                    .child(
                        ui::button("Nested enable", ButtonVariant::Quiet).on_press(move || {
                            spawn(async move {
                                outcome.set(format!(
                                    "permission {:?}",
                                    state.request_permission().await
                                ));
                            });
                        }),
                    )
            },
        ))
}

#[test]
fn overlapping_sends_keep_their_results_and_duplicate_acknowledgements_are_ignored()
-> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<NotificationHost>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("text:pending 0")?;
    app.press("Send first")?;
    app.wait_for("text:pending 1")?;
    app.press("Send second")?;
    app.wait_for("text:pending 2")?;
    app.press("Ack second")?;
    app.wait_for("second Ok(())")?;
    app.wait_for("text:pending 1")?;
    app.press("Ack second")?;
    app.wait_for("text:pending 1")?;
    app.press("Reject first")?;
    app.wait_for("first Err(\"native refused\")")?;
    app.wait_for("text:pending 0")?;
    Ok(())
}

#[test]
fn an_unresponsive_host_has_a_bounded_queue_and_reports_capacity() -> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<NotificationHost>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("text:pending 0")?;
    app.press("Flood")?;
    app.wait_for(&format!("text:pending {MAX_PENDING}"))?;
    app.wait_for("Too many pending notification requests")?;
    Ok(())
}

#[test]
fn permission_request_from_a_later_event_reaches_the_host() -> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<NotificationHost>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("idle")?;
    app.press("Nested enable")?;
    app.wait_for("text:pending 1")?;
    app.wait_for("text:other pending 0")?;
    app.press("Deny")?;
    app.wait_for("permission Ok(false)")?;
    app.wait_for("text:pending 0")?;
    Ok(())
}
