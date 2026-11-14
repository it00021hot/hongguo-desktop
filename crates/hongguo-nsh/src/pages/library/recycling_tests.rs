use super::{history, model::HistoryItem, tasks};
use hongguo_common::model::{DownloadTask, QueueStatus};
use nsh_core::{Checked, element::ImageSrc, prelude::*};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

#[component]
fn RememberedHistory() -> impl IntoElement {
    crate::view_memory::provide();
    let shown = signal(true);
    view()
        .flex_1()
        .child(
            crate::ui::button("Leave history", crate::ui::ButtonVariant::Quiet)
                .on_press(move || shown.set(false)),
        )
        .child(
            crate::ui::button("Return history", crate::ui::ButtonVariant::Quiet)
                .on_press(move || shown.set(true)),
        )
        .child(when(shown, || HistoryRecycling::element(())))
}

#[test]
fn history_scroll_survives_page_remount_and_resets_on_tab_change() -> Result<(), String> {
    let mut app = start::<RememberedHistory>()?;
    app.set_viewport(size(px(800.0), px(600.0)))?;
    app.wait_for("History 0")?;
    app.scroll("id:history-records", px(5200.0))?;
    app.press("Leave history")?;
    app.wait_for_gone("id:history-records", Duration::from_secs(1))?;
    app.press("Return history")?;
    app.wait_for("id:history-records")?;
    app.scroll("id:history-records", px(-5200.0))?;
    assert!(app.scroll("id:history-records", px(-1.0)).is_err());
    app.scroll("id:history-records", px(5200.0))?;
    app.press("Unfinished")?;
    app.wait(Duration::from_millis(32))?;
    assert!(app.scroll("id:history-records", px(-1.0)).is_err());
    Ok(())
}

#[component]
fn HistoryRecycling() -> impl IntoElement {
    let opened = signal(String::new());
    let items = signal(
        (0..100)
            .map(|index| HistoryItem {
                id: index.to_string(),
                vid: format!("vid-{index}"),
                title: format!("History {index}"),
                cover: ImageSrc::from(""),
                episode: 1,
                episodes: 80,
                position_ms: 0,
                duration_ms: 1,
                updated_at_ms: 0,
            })
            .collect::<Vec<_>>(),
    );
    view()
        .flex_1()
        .child(history::content(
            items,
            false,
            0,
            history::HistoryEvents {
                open: Box::new(move |item| opened.set(format!("opened:{}", item.id))),
                remove: None,
                busy: computed(|| false),
                reset: computed(|| (0, 0)),
            },
        ))
        .child(text(move || opened.get()))
}

#[test]
fn history_scrollbar_uses_the_page_edge_and_rows_keep_their_inset() -> Result<(), String> {
    let mut app = start::<HistoryRecycling>()?;
    app.set_viewport(size(px(800.0), px(600.0)))?;
    app.wait_for("History 0")?;
    let nodes = app.screen().layout_nodes();
    let list = nodes
        .iter()
        .find(|node| node.test_id.as_deref() == Some("history-records"))
        .ok_or("history list missing")?;
    assert_eq!(list.bounds.origin.x.0, 0.0);
    assert_eq!(list.bounds.size.width.0, 800.0);
    let title = nodes
        .iter()
        .find(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text.text == "History 0")
        })
        .ok_or("history title missing")?;
    assert_eq!(title.bounds.origin.x.0, 117.0);
    Ok(())
}

#[component]
fn TasksRecycling() -> impl IntoElement {
    let items = signal(
        (0..100)
            .map(|index| {
                let mut task =
                    DownloadTask::new("series", &format!("Task {index}"), 1, "vid", "Episode");
                task.id = index.to_string();
                task
            })
            .collect::<Vec<_>>(),
    );
    tasks::content(
        items,
        QueueStatus::default,
        false,
        tasks::TaskCapabilities::PRODUCTION,
        |_| {
            panic!("selecting a local row must not dispatch a download action");
        },
    )
}

fn start<C: nsh_core::element::Component>() -> Result<Headless, String>
where
    C::Props: Default,
{
    let mut config = EngineConfig::new(Root::of::<C>().tokens(nsh_base::tokens_of(Some(
        include_str!("../../../theme.json"),
    ))?));
    config.environment.os = nsh_core::platform::Os::MacOs;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.set_viewport(size(px(800.0), px(360.0)))?;
    Ok(app)
}

#[test]
fn history_cells_follow_recycled_positions_and_open_current_identity() -> Result<(), String> {
    let mut app = start::<HistoryRecycling>()?;
    app.wait_for("History 0")?;
    app.scroll("id:history-records", px(5200.0))?;
    app.wait_for("History 40")?;
    app.press("role:button[History 40]")?;
    app.wait_for("text:opened:40")?;
    Ok(())
}

#[test]
fn task_cell_selection_stays_with_identity_after_recycling() -> Result<(), String> {
    let mut app = start::<TasksRecycling>()?;
    app.wait_for("Task 0")?;
    app.scroll("id:task-records", px(1060.0))?;
    app.wait_for("Task 20")?;
    app.press("role:checkbox[20]")?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:checkbox[20]")?
            .checked,
        Some(Checked::True)
    );
    app.scroll("id:task-records", px(-1060.0))?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:checkbox[0]")?
            .checked,
        Some(Checked::False)
    );
    app.scroll("id:task-records", px(1060.0))?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:checkbox[20]")?
            .checked,
        Some(Checked::True)
    );
    Ok(())
}
