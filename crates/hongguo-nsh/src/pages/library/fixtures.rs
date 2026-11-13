//! Offline review data shared by the production presentation components.
//! Keep in sync with target/tauri-layout-reference/library-reference.ts during review.
use super::{
    history as history_view,
    model::HistoryItem,
    tasks::{self, TaskCapabilities, TaskIntent},
};
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_common::model::{DownloadTask, QueueStatus, TaskStatus};
use nsh_core::{element::ImageSrc, prelude::*};

const FIXTURE_TIME: i64 = 1_794_355_200_000; // 2026-11-11 UTC
const TITLE: &str = "装穷提亲，娶到全家嫌弃的她";

pub(in crate::pages) fn history(nav: Navigation, cover: ImageSrc) -> Screen {
    let items = [
        ("fixture", TITLE, 1, 80, 42_000, 169_000),
        (
            "preview-history-finished",
            "故事的最后一集",
            24,
            24,
            120_000,
            150_000,
        ),
        ("preview-history-next", "下一段故事", 3, 60, 15_000, 180_000),
    ]
    .map(
        |(id, title, episode, episodes, position_ms, duration_ms)| HistoryItem {
            id: id.into(),
            vid: format!("offline-{episode}"),
            title: title.into(),
            cover: cover.clone(),
            episode,
            episodes,
            position_ms,
            duration_ms,
            updated_at_ms: FIXTURE_TIME,
        },
    )
    .to_vec();
    let items = signal(items);
    let deleted = signal(0_u64);
    ui::page(nav, move || tr("nav.history.title")).child(history_view::content(
        items,
        true,
        FIXTURE_TIME,
        history_view::HistoryEvents {
            open: Box::new(move |item| nav.push(Route::Player(item.id.clone(), item.episode))),
            remove: Some(Box::new(move |entries| {
                items.update(|items| {
                    items.retain(|item| !entries.iter().any(|entry| entry.id == item.id))
                });
                deleted.update(|revision| *revision += 1);
            })),
            busy: computed(|| false),
            reset: computed(move || (0, deleted.get())),
        },
    ))
}

pub(in crate::pages) fn tasks(nav: Navigation) -> Screen {
    let items = signal(
        [
            TaskStatus::Running,
            TaskStatus::Pending,
            TaskStatus::Completed,
            TaskStatus::Failed,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, status)| DownloadTask {
            id: format!("preview-task-{}", index + 1),
            series_id: "fixture".into(),
            series_title: TITLE.into(),
            vid_index: index as u32 + 1,
            vid: format!("offline-episode-{}", index + 1),
            ep_title: format!("第{}集", index + 1),
            file_path: String::new(),
            temp_path: String::new(),
            status,
            downloaded: match status {
                TaskStatus::Running => 4 * 1024 * 1024,
                TaskStatus::Completed => 8 * 1024 * 1024,
                _ => 0,
            },
            total: 8 * 1024 * 1024,
            error: if status == TaskStatus::Failed {
                "error.network".into()
            } else {
                String::new()
            },
            created_at: FIXTURE_TIME,
            updated_at: FIXTURE_TIME,
        })
        .collect::<Vec<_>>(),
    );
    let feedback = signal(SharedString::default());
    ui::page(nav, move || tr("nav.tasks.title")).child(
        view()
            .flex_1()
            .child(tasks::content(
                items,
                move || QueueStatus {
                    running: 1,
                    pending: 1,
                    completed: 1,
                    failed: 1,
                    limit: 3,
                    active: 1,
                },
                false,
                TaskCapabilities::PREVIEW,
                move |intent| match intent {
                    TaskIntent::Pause => items.update(|items| {
                        for item in items {
                            if item.status == TaskStatus::Running {
                                item.status = TaskStatus::Stopped;
                            }
                        }
                    }),
                    TaskIntent::Resume => items.update(|items| {
                        for item in items {
                            if item.status == TaskStatus::Stopped {
                                item.status = TaskStatus::Pending;
                            }
                        }
                    }),
                    TaskIntent::Retry(ids) => items.update(|items| {
                        for item in items {
                            if ids.contains(&item.id) {
                                item.reset_for_retry();
                            }
                        }
                    }),
                    TaskIntent::Stop(id) => items.update(|items| {
                        for item in items {
                            if item.id == id {
                                item.status = TaskStatus::Stopped;
                            }
                        }
                    }),
                    TaskIntent::Delete(ids, _) => {
                        items.update(|items| items.retain(|item| !ids.contains(&item.id)))
                    }
                    TaskIntent::Remove(_) => unreachable!("removal requires confirmation"),
                    TaskIntent::Folder(id) => feedback.set(
                        format!(
                            "{} · {id}",
                            tr("nsh.previewAction").replace(
                                "{action}",
                                &tr(if is_desktop() {
                                    "tasks.actions.openFolder"
                                } else {
                                    "nsh.exportFile"
                                })
                            )
                        )
                        .into(),
                    ),
                    TaskIntent::Rescan => feedback.set(tr("tasks.rescanNone")),
                },
            ))
            .child(ui::status(feedback)),
    )
}
