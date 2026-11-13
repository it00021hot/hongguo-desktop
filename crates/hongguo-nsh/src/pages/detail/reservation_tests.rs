use super::reservation;
use crate::{bridge, ui};
use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_testing::Headless;
use std::time::{Duration, SystemTime};

#[component]
fn Actions() -> impl IntoElement {
    let loaded = resource(|| Some(()), |_, _| std::future::ready(Ok(false)));
    let identity = signal(0_u64);
    let fail = signal(false);
    let calls = signal(0_u32);
    let state = reservation::controller(
        loaded,
        computed(move || identity.get()),
        || true,
        move |_| {
            let fail = fail.get_untracked();
            calls.update(|value| *value += 1);
            async move {
                timer(Duration::from_millis(200)).await;
                if fail {
                    Err(bridge::Failure::from(hongguo_common::AppError::Auth(
                        "Reservation rejected".into(),
                    )))
                } else {
                    Ok(())
                }
            }
        },
        || {},
    );
    view()
        .child(ui::reserve_action(state.selected, state.busy).on_press(move || state.submit()))
        .child(text(state.message))
        .child(text(move || format!("requests:{}", calls.get())))
        .child(ui::button("Reject next", ui::ButtonVariant::Quiet).on_press(move || fail.set(true)))
        .child(
            ui::button("Switch identity", ui::ButtonVariant::Quiet)
                .on_press(move || identity.update(|value| *value += 1)),
        )
}

fn start() -> Headless {
    Headless::start(EngineConfig {
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            Root::of::<Actions>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
        )
    })
    .unwrap()
}

#[test]
fn a_write_is_single_flight_and_rejection_keeps_the_previous_membership() -> Result<(), String> {
    let mut app = start();
    app.wait_for("role:button[Reserve]")?;
    app.press("role:button[Reserve]")?;
    assert!(!app.screen().enabled("role:button[Reserve]")?);
    app.run_for(Duration::from_millis(250))?;
    app.wait_for("role:button[Reserved]")?;
    app.press("Reject next")?;
    app.press("role:button[Reserved]")?;
    app.run_for(Duration::from_millis(250))?;
    assert!(app.screen().matches("role:button[Reserved]")?);
    assert!(app.shows("Reservation rejected"));
    assert!(app.shows("requests:2"));
    Ok(())
}

#[test]
fn a_write_for_the_previous_identity_cannot_select_the_new_accounts_button() -> Result<(), String> {
    let mut app = start();
    app.wait_for("role:button[Reserve]")?;
    app.press("role:button[Reserve]")?;
    app.press("Switch identity")?;
    app.run_for(Duration::from_millis(250))?;
    assert!(app.screen().enabled("role:button[Reserve]")?);
    assert!(!app.screen().matches("role:button[Reserved]")?);
    Ok(())
}
