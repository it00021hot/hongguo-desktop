use super::{
    history,
    model::HistoryItem,
    tasks::{self, TaskCapabilities, TaskIntent},
};
use hongguo_common::model::{DownloadTask, QueueStatus, TaskStatus};
use nsh_core::{element::ImageSrc, prelude::*};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn DownloadRemoval() -> impl IntoElement {
    let dispatched = signal(String::new());
    let mut task = DownloadTask::new("series", "Completed episode", 1, "vid", "Episode");
    task.id = "done".into();
    task.status = TaskStatus::Completed;
    task.total = 8 * 1024 * 1024;
    let items = signal(vec![task]);
    view()
        .flex_1()
        .child(tasks::content(
            items,
            QueueStatus::default,
            false,
            TaskCapabilities::PRODUCTION,
            move |intent| {
                if let TaskIntent::Delete(ids, files) = intent {
                    dispatched.set(format!("deleted:{}:{files}", ids.join(",")));
                    items.set(Vec::new());
                } else {
                    panic!("confirmation must dispatch only a confirmed deletion");
                }
            },
        ))
        .child(text(move || dispatched.get()))
}

#[component]
fn DownloadExport() -> impl IntoElement {
    let selected = signal(String::new());
    let mut task = DownloadTask::new(
        "shared-series",
        "Same series",
        2,
        "second-video",
        "Episode two",
    );
    task.id = "second-task".into();
    task.status = TaskStatus::Completed;
    view()
        .flex_1()
        .child(tasks::content(
            move || vec![task.clone()],
            QueueStatus::default,
            false,
            TaskCapabilities::PRODUCTION,
            move |intent| {
                if let TaskIntent::Folder(id) = intent {
                    selected.set(id);
                }
            },
        ))
        .child(text(move || format!("export:{}", selected.get())))
}

#[test]
fn mobile_export_targets_the_clicked_episode_instead_of_the_first_file_of_its_series()
-> Result<(), String> {
    let mut config = EngineConfig::new(Root::of::<DownloadExport>().tokens(nsh_base::tokens_of(
        Some(include_str!("../../../theme.json")),
    )?));
    config.environment.os = nsh_core::platform::Os::Android;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.set_viewport(size(px(1000.0), px(700.0)))?;
    app.wait_for("Same series")?;
    app.press("role:button[Export file]")?;
    app.wait_for("text:export:second-task")?;
    Ok(())
}

#[component]
fn HistoryRemoval() -> impl IntoElement {
    let items = signal(vec![HistoryItem {
        id: "series".into(),
        vid: "episode".into(),
        title: "Local history".into(),
        cover: ImageSrc::from(""),
        episode: 1,
        episodes: 80,
        position_ms: 0,
        duration_ms: 1,
        updated_at_ms: 0,
    }]);
    let revision = signal(0);
    let removed = signal(String::new());
    view()
        .flex_1()
        .child(history::content(
            items,
            true,
            0,
            history::HistoryEvents {
                open: Box::new(|_| panic!("management clicks must select instead of playing")),
                remove: Some(Box::new(move |entries| {
                    removed.set(
                        entries
                            .iter()
                            .map(|entry| entry.vid.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    items.set(Vec::new());
                    revision.update(|value| *value += 1);
                })),
                busy: computed(|| false),
                reset: computed(move || (0, revision.get())),
            },
        ))
        .child(text(move || format!("removed:{}", removed.get())))
}

fn start<C: Component>() -> Result<Headless, String>
where
    C::Props: Default,
{
    let mut config = EngineConfig::new(Root::of::<C>().tokens(nsh_base::tokens_of(Some(
        include_str!("../../../theme.json"),
    ))?));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.set_viewport(size(px(1000.0), px(700.0)))?;
    Ok(app)
}

#[test]
fn task_removal_requires_confirmation_and_preserves_explicit_file_choice() -> Result<(), String> {
    let mut app = start::<DownloadRemoval>()?;
    app.wait_for("Completed episode")?;
    app.press("role:button[Delete]")?;
    app.wait_for("role:button[Delete records only]")?;
    assert!(!app.shows("text:deleted:done:true"));
    app.press_key("Escape")?;
    app.wait_for_gone(
        "role:button[Delete records only]",
        std::time::Duration::from_secs(1),
    )?;
    app.press("role:button[Delete]")?;
    app.press("role:checkbox[Also delete local files]")?;
    app.press("role:button[Delete tasks and files]")?;
    app.wait_for("text:deleted:done:true")?;
    Ok(())
}

#[test]
fn history_management_selects_and_removes_only_after_confirmation() -> Result<(), String> {
    let mut app = start::<HistoryRemoval>()?;
    app.wait_for("Local history")?;
    app.press("role:button[Manage]")?;
    app.press("role:button[Local history]")?;
    app.press("role:button[Delete selected]")?;
    app.wait_for("text:Delete selected watch history?")?;
    app.press("role:button[Cancel]")?;
    app.wait_for("Local history")?;
    assert!(!app.shows("text:removed:episode"));
    app.press("role:button[Delete selected]")?;
    app.press("id:confirmation >> role:button[Confirm]")?;
    app.wait_for("text:removed:episode")?;
    app.wait_for("text:Dramas watched in the official app or here show up here")?;
    assert!(!app.shows("role:button[Manage]"));
    assert!(!app.shows("text:Signed out — watch history is saved on this device and syncs to the cloud after you sign in"));
    Ok(())
}
