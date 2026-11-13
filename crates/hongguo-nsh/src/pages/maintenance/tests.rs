use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Environment, Root, Size};
use nsh_testing::{DialogAnswer, Headless};
use std::time::SystemTime;

#[component]
fn Merge() -> impl IntoElement {
    let nav = crate::navigation::Navigation::new();
    nav.push(crate::navigation::Route::Merge);
    crate::ui::stack().child(super::merge::preview(nav))
}

#[component]
fn Storage() -> impl IntoElement {
    let nav = crate::navigation::Navigation::new();
    nav.push(crate::navigation::Route::Storage);
    crate::ui::stack().child(super::storage::preview(nav))
}

fn start(root: Root) -> Headless {
    let mut app = Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::MacOs,
            ..Default::default()
        },
        viewport: Size::new(px(1280.0), px(840.0)),
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            root.tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
        )
    })
    .unwrap();
    app.answer_dialogs_myself();
    app
}

fn select(app: &mut Headless, key: &str) -> Result<(), String> {
    app.press("role:combobox[Series]")?;
    app.press_key(key)?;
    app.press_key("enter")
}

#[test]
fn choosing_a_series_derives_output_and_mismatch_selects_the_actual_hevc_mode() -> Result<(), String>
{
    let mut app = start(Root::of::<Merge>());
    app.wait_for("id:merge-series")?;
    assert!(!app.screen().enabled("role:button[Start merge]")?);
    select(&mut app, "home")?;
    assert_eq!(
        app.screen().input_text("id:merge-output")?,
        Some("装穷提亲，娶到全家嫌弃的她")
    );
    assert!(app.screen().selected("role:tab[Quick merge]")?);
    select(&mut app, "end")?;
    assert_eq!(
        app.screen().input_text("id:merge-output")?,
        Some("下一段故事")
    );
    assert!(!app.screen().enabled("role:tab[Quick merge]")?);
    assert!(app.screen().selected("role:tab[HEVC merge]")?);
    assert!(!app.shows("0 B free"));
    select(&mut app, "home")?;
    assert!(app.screen().selected("role:tab[Quick merge]")?);
    Ok(())
}

#[test]
fn starting_once_adds_a_running_record_and_prevents_the_same_output_starting_twice()
-> Result<(), String> {
    let mut app = start(Root::of::<Merge>());
    app.wait_for("id:merge-series")?;
    select(&mut app, "home")?;
    app.press("role:button[Start merge]")?;
    app.wait_for("text:Merge started")?;
    assert!(!app.screen().enabled("role:button[Merging…]")?);
    assert_eq!(
        app.screen()
            .count("id:merge-tasks >> role:button[Delete this merge record]")?,
        4
    );
    Ok(())
}

#[test]
fn cancelling_merge_record_removal_keeps_it_and_confirming_removes_only_that_record()
-> Result<(), String> {
    let mut app = start(Root::of::<Merge>());
    app.wait_for("id:merge-tasks")?;
    let delete = "id:merge-tasks >> role:button[Delete this merge record] >> nth=0";
    app.press(delete)?;
    assert!(app.pending_dialog().is_some());
    app.answer_dialog(DialogAnswer::Press("Cancel".into()))?;
    assert!(app.shows("装穷提亲 · 兼容版"));
    app.press(delete)?;
    app.answer_dialog(DialogAnswer::Press("Delete".into()))?;
    assert!(!app.shows("装穷提亲 · 兼容版"));
    assert!(app.shows("装穷提亲 · 快速版"));
    assert_eq!(
        app.screen()
            .count("id:merge-tasks >> role:button[Delete this merge record]")?,
        2
    );
    Ok(())
}

#[test]
fn storage_filter_matches_titles_and_deleting_one_series_updates_the_real_totals()
-> Result<(), String> {
    let mut app = start(Root::of::<Storage>());
    app.wait_for("id:storage-search")?;
    app.wait_for("text:768.0 MB")?;
    app.type_text("id:storage-search", "下一段")?;
    assert!(app.shows("下一段故事"));
    assert!(!app.shows("归途有风"));
    app.clear("id:storage-search")?;
    app.press("role:button[Delete local files of 下一段故事]")?;
    app.wait_for("text:576.0 MB")?;
    assert!(app.shows("12 files"));
    assert!(!app.screen().matches("id:storage-rows >> text:下一段故事")?);
    assert!(app.shows("归途有风"));
    Ok(())
}

#[test]
fn storage_clear_all_requires_confirmation_and_cancel_keeps_usage() -> Result<(), String> {
    let mut app = start(Root::of::<Storage>());
    app.wait_for("id:storage-search")?;
    app.press("role:button[Delete everything downloaded]")?;
    assert!(app.pending_dialog().is_some());
    app.answer_dialog(DialogAnswer::Press("Cancel".into()))?;
    assert!(app.shows("768.0 MB"));
    app.press("role:button[Delete everything downloaded]")?;
    app.answer_dialog(DialogAnswer::Press("Confirm".into()))?;
    app.wait_for("text:0 B")?;
    assert!(app.shows("0 files"));
    assert_eq!(app.screen().count("id:storage-rows >> role:button")?, 0);
    Ok(())
}
