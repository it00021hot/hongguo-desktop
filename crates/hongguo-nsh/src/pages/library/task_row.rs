use super::tasks::{TaskCapabilities, TaskIntent};
use crate::{
    i18n::{format_bytes as bytes, tr},
    ui,
};
use hongguo_common::model::{DownloadTask, TaskStatus};
use nsh_base::theme;
use nsh_core::prelude::*;
use std::collections::HashSet;

pub(super) fn build(
    item: Memo<Option<DownloadTask>>,
    busy: Memo<bool>,
    capabilities: TaskCapabilities,
    selected: Signal<HashSet<String>>,
    compact: Memo<bool>,
    send: impl Fn(TaskIntent) + Copy + 'static,
) -> AnyElement {
    ui::task_table_row(false)
        .style_when(move || item.get().is_none(), |style| style.hidden())
        .style_when(
            move || {
                item.get()
                    .is_some_and(|item| selected.with(|selected| selected.contains(&item.id)))
            },
            |style| style.bg(move || theme().muted),
        )
        .child(
            ui::task_cell(ui::TaskColumn::Select, compact).child(
                ui::library_check(
                    move || {
                        item.get()
                            .map(|item| SharedString::from(item.id))
                            .unwrap_or_default()
                    },
                    move || {
                        item.get().is_some_and(|item| {
                            selected.with(|selected| selected.contains(&item.id))
                        })
                    },
                    false,
                )
                .on_change(move |_: &bool| {
                    selected.update(|selected| {
                        let Some(id) = item.get_untracked().map(|item| item.id) else {
                            return;
                        };
                        if !selected.remove(&id) {
                            selected.insert(id);
                        }
                    })
                }),
            ),
        )
        .child(
            ui::task_cell(ui::TaskColumn::Title, compact)
                .child(
                    text(move || item.get().map(|item| item.series_title).unwrap_or_default())
                        .text_sm()
                        .line_height(px(20.0))
                        .font_weight(FontWeight::NORMAL)
                        .number_of_lines(1),
                )
                .child(when(
                    move || item.get().is_some_and(|item| !item.error.is_empty()),
                    move || {
                        text(move || item.get().map(|item| tr(&item.error)).unwrap_or_default())
                            .text_xs()
                            .line_height(px(16.0))
                            .font_weight(FontWeight::NORMAL)
                            .number_of_lines(1)
                            .text_color(move || theme().destructive)
                    },
                )),
        )
        .child(
            ui::task_cell(ui::TaskColumn::Episode, compact).child(
                text(move || {
                    item.get()
                        .map(|item| item.vid_index.to_string())
                        .unwrap_or_default()
                })
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::NORMAL),
            ),
        )
        .child(
            ui::task_cell(ui::TaskColumn::Status, compact).child(dynamic(move || {
                let Some(status) = item.get().map(|item| item.status) else {
                    return view().into_element();
                };
                ui::task_badge(status, move || status_label(status)).into_element()
            })),
        )
        .child(
            ui::task_cell(ui::TaskColumn::Size, compact).child(dynamic(move || {
                let Some(item) = item.get() else {
                    return view().into_element();
                };
                let size = text(if item.status == TaskStatus::Running {
                    format!("{} / {}", bytes(item.downloaded), bytes(item.total))
                } else {
                    bytes(item.total)
                })
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL)
                .font_family("monospace")
                .self_start()
                .number_of_lines(1)
                .text_overflow(nsh_core::EllipsizeMode::Clip);
                if item.status == TaskStatus::Running {
                    view()
                        .gap_1()
                        .child(ui::task_progress(item.percent()))
                        .child(size.text_color(move || theme().muted_foreground))
                        .into_element()
                } else {
                    size.into_element()
                }
            })),
        )
        .child(
            ui::task_cell(ui::TaskColumn::Actions, compact).p_2().child(
                view()
                    .flex_row()
                    .gap_1()
                    .child(when(
                        move || {
                            item.get()
                                .is_some_and(|item| item.status == TaskStatus::Running)
                        },
                        move || {
                            ui::library_icon(ui::Icon::Pause, move || tr("tasks.actions.stop"))
                                .disabled(move || busy.get() || !capabilities.stop)
                                .on_press(move || {
                                    if let Some(item) = item.get_untracked() {
                                        send(TaskIntent::Stop(item.id));
                                    }
                                })
                        },
                    ))
                    .child(when(
                        move || {
                            item.get().is_some_and(|item| {
                                matches!(item.status, TaskStatus::Failed | TaskStatus::Stopped)
                            })
                        },
                        move || {
                            ui::library_icon(ui::Icon::Retry, move || tr("tasks.actions.retry"))
                                .disabled(move || busy.get() || !capabilities.retry)
                                .on_press(move || {
                                    if let Some(item) = item.get_untracked() {
                                        send(TaskIntent::Retry(vec![item.id]));
                                    }
                                })
                        },
                    ))
                    .child(when(
                        move || {
                            item.get()
                                .is_some_and(|item| item.status == TaskStatus::Completed)
                        },
                        move || {
                            ui::library_icon(ui::Icon::Folder, move || {
                                tr(if is_desktop() {
                                    "tasks.actions.openFolder"
                                } else {
                                    "nsh.exportFile"
                                })
                            })
                            .disabled(move || busy.get() || !capabilities.folder)
                            .on_press(move || {
                                if let Some(item) = item.get_untracked() {
                                    send(TaskIntent::Folder(item.id));
                                }
                            })
                        },
                    ))
                    .child(
                        ui::library_icon(ui::Icon::Delete, move || tr("tasks.actions.remove"))
                            .disabled(move || busy.get() || !capabilities.remove)
                            .on_press(move || {
                                if let Some(item) = item.get_untracked() {
                                    send(TaskIntent::Remove(vec![item.id]));
                                }
                            }),
                    ),
            ),
        )
        .into_element()
}

fn status_label(status: TaskStatus) -> SharedString {
    tr(match status {
        TaskStatus::Running => "tasks.status.running",
        TaskStatus::Pending => "tasks.status.pending",
        TaskStatus::Completed => "tasks.status.completed",
        TaskStatus::Failed => "tasks.status.failed",
        TaskStatus::Stopped => "tasks.status.stopped",
    })
}
