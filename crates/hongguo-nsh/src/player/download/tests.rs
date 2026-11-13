use super::*;
use nsh_engine::{EngineConfig, Root, Size};
use nsh_testing::Headless;

#[component]
fn Download() -> impl IntoElement {
    let open = signal(false);
    let submitted = signal(String::new());
    view()
        .flex_1()
        .child(
            ui::button("Open download", ui::ButtonVariant::Primary)
                .on_press(move || open.set(true)),
        )
        .child(text(move || submitted.get()))
        .child(presenter(
            open,
            computed(|| {
                Some(Target {
                    id: "series".into(),
                    indices: (1..=80).collect(),
                    current: 7,
                })
            }),
            move |_, indices, busy, message| {
                busy.set(false);
                submitted.set(format!("submitted:{}", range::format_range(&indices)));
                message.set("Upload failed".into());
            },
        ))
}

fn start() -> Headless {
    let mut config = EngineConfig::new(
        Root::of::<Download>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).expect("theme")),
    );
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.reduce_motion = true;
    config.viewport = Size::new(px(1280.0), px(840.0));
    Headless::start(config).expect("download fixture")
}

#[test]
fn range_errors_keep_selection_and_failed_submit_preserves_dialog_and_exact_ids()
-> Result<(), String> {
    let mut app = start();
    app.wait_for("Open download")?;
    app.press("Open download")?;
    app.wait_for("1 selected")?;
    assert!(app.screen().selected("role:button[Select episodes 7]")?);
    app.type_text("id:download-range", "1-3,5，7")?;
    app.wait_for("5 selected")?;
    app.press("role:button[Start download]")?;
    app.wait_for("text:submitted:1-3, 5, 7")?;
    app.wait_for("Upload failed")?;
    assert_eq!(app.screen().count("id:player-download-sheet")?, 1);
    app.clear("id:download-range")?;
    app.press("role:button[First 10]")?;
    app.type_text("id:download-range", "bad")?;
    app.wait_for("Could not parse the range")?;
    app.wait_for("10 selected")?;
    Ok(())
}

#[test]
fn close_discards_selection_and_reopen_starts_at_current_episode() -> Result<(), String> {
    let mut app = start();
    app.wait_for("Open download")?;
    app.press("Open download")?;
    app.wait_for("1 selected")?;
    app.press("role:button[Select all]")?;
    app.wait_for("80 selected")?;
    app.press("role:button[Clear]")?;
    app.wait_for("0 selected")?;
    assert!(!app.screen().enabled("role:button[Start download]")?);
    app.press_key("Escape")?;
    app.wait_for_gone(
        "id:player-download-sheet",
        std::time::Duration::from_secs(1),
    )?;
    app.press("Open download")?;
    app.wait_for("1 selected")?;
    assert!(app.screen().selected("role:button[Select episodes 7]")?);
    Ok(())
}
