use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

#[component]
fn HistoryModels() -> impl IntoElement {
    crate::library::cache_tests::window(|session, nav| {
        // Keep the source inactive to inspect the resource without an unrelated
        // refresh replacing the seeded value on remount.
        let result = data(session, nav);
        let managing = signal(false);
        view()
            .test_id("history-model")
            .child(text(move || {
                format!(
                    "history-owner:{:?}",
                    result.with(|data| data.map(|(owner, _)| *owner))
                )
            }))
            .child(text(move || format!("managing:{}", managing.get())))
            .child(
                ui::button("Seed history model", ui::ButtonVariant::Quiet).on_press(move || {
                    result.mutate((session.identity(), Vec::new()));
                    managing.set(true);
                }),
            )
            .into_element()
    })
}

#[test]
fn loaded_history_resource_survives_page_disposal_without_retaining_management()
-> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<HistoryModels>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("id:history-model")?;
    app.press("Seed history model")?;
    app.wait_for("text:history-owner:Some(0)")?;
    app.press("Leave model")?;
    app.wait_for_gone("id:history-model", Duration::from_secs(1))?;
    app.press("Return model")?;
    app.wait_for("text:history-owner:Some(0)")?;
    app.wait_for("text:managing:false")?;
    Ok(())
}
