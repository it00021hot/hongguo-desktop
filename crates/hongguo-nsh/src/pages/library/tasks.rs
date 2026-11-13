use crate::{i18n::tr, ui};
use hongguo_common::model::{DownloadTask, QueueStatus, TaskStatus};
use nsh_base::theme;
use nsh_core::prelude::*;
use std::collections::HashSet;

#[derive(Clone)]
pub(super) enum TaskIntent {
    Resume,
    Pause,
    Retry(Vec<String>),
    Stop(String),
    Folder(String),
    Remove(Vec<String>),
    Delete(Vec<String>, bool),
    Rescan,
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn production_queue_management_uses_available_service_operations() {
        let capabilities = TaskCapabilities::PRODUCTION;
        assert!(capabilities.can(&TaskIntent::Remove(vec!["task".into()])));
        assert!(capabilities.can(&TaskIntent::Rescan));
    }
}

#[derive(Clone, Copy)]
pub(super) struct TaskCapabilities {
    pub queue: bool,
    pub retry: bool,
    pub stop: bool,
    pub folder: bool,
    pub remove: bool,
    pub rescan: bool,
}

impl TaskCapabilities {
    pub const PREVIEW: Self = Self {
        queue: true,
        retry: true,
        stop: true,
        folder: true,
        remove: true,
        rescan: true,
    };
    pub const PRODUCTION: Self = Self {
        queue: true,
        retry: true,
        stop: true,
        folder: true,
        remove: true,
        rescan: true,
    };
    pub fn can(self, intent: &TaskIntent) -> bool {
        match intent {
            TaskIntent::Resume | TaskIntent::Pause => self.queue,
            TaskIntent::Retry(_) => self.retry,
            TaskIntent::Stop(_) => self.stop,
            TaskIntent::Folder(_) => self.folder,
            TaskIntent::Remove(_) | TaskIntent::Delete(_, _) => self.remove,
            TaskIntent::Rescan => self.rescan,
        }
    }
}

pub(super) fn content(
    items: impl IntoReactive<Vec<DownloadTask>>,
    status: impl IntoReactive<QueueStatus>,
    busy: impl IntoReactive<bool>,
    capabilities: TaskCapabilities,
    dispatch: impl Fn(TaskIntent) + 'static,
) -> View {
    let items = items.into_reactive();
    let items = computed(move || items.get());
    let status = status.into_reactive();
    let busy = busy.into_reactive();
    let busy = computed(move || busy.get());
    let dispatch = stored(dispatch);
    let selected = signal(HashSet::<String>::new());
    let removing = signal(Vec::<String>::new());
    let confirmation = signal(false);
    let with_files = signal(false);
    let ids = computed(move || {
        items
            .get()
            .into_iter()
            .map(|item| item.id)
            .collect::<HashSet<_>>()
    });
    let failed = computed(move || {
        items
            .get()
            .into_iter()
            .filter(|item| item.status == TaskStatus::Failed)
            .map(|item| item.id)
            .collect::<Vec<_>>()
    });
    let selected_count = computed(move || {
        selected.with(|selection| ids.with(|ids| selection.intersection(ids).count()))
    });
    let all_checked =
        computed(move || !ids.get().is_empty() && selected_count.get() == ids.get().len());
    let freeable = computed(move || {
        items.with(|items| {
            items
                .iter()
                .filter(|item| {
                    item.status == TaskStatus::Completed
                        && selected.with(|ids| ids.contains(&item.id))
                })
                .map(|item| item.total)
                .sum::<u64>()
        })
    });
    let toggle_all = move || {
        selected.set(if all_checked.get_untracked() {
            HashSet::new()
        } else {
            ids.get_untracked()
        })
    };
    let send = move |intent: TaskIntent| {
        if !busy.get_untracked() && capabilities.can(&intent) {
            if let TaskIntent::Remove(ids) = intent {
                removing.set(ids);
                with_files.set(false);
                confirmation.set(true);
                return;
            }
            dispatch.with(|dispatch| dispatch(intent));
        }
    };
    let toolbar = view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            text(move || {
                let status = status.get();
                let counts = tr("tasks.queueStatus")
                    .replace("{running}", &status.running.to_string())
                    .replace("{pending}", &status.pending.to_string())
                    .replace("{completed}", &status.completed.to_string())
                    .replace("{failed}", &status.failed.to_string());
                let concurrency = tr("tasks.concurrency")
                    .replace("{active}", &status.active.to_string())
                    .replace("{limit}", &status.limit.to_string());
                format!("{counts} · {concurrency}")
            })
            .text_sm()
            .line_height(px(20.0))
            .font_weight(FontWeight::NORMAL)
            .text_color(move || theme().muted_foreground),
        )
        .child(view().flex_1())
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .max_w(percentage(1.0))
                .min_w(px(0.0))
                .gap_2()
                .child(
                    ui::library_action(move || tr("tasks.startAll"), Some(ui::Icon::Play))
                        .disabled(move || busy.get() || !capabilities.queue)
                        .on_press(move || send(TaskIntent::Resume)),
                )
                .child(
                    ui::library_action(move || tr("tasks.pauseAll"), Some(ui::Icon::Pause))
                        .disabled(move || busy.get() || !capabilities.queue)
                        .on_press(move || send(TaskIntent::Pause)),
                )
                .child(
                    ui::library_action(move || tr("tasks.retryAll"), Some(ui::Icon::Retry))
                        .disabled(move || {
                            busy.get() || !capabilities.retry || failed.get().is_empty()
                        })
                        .on_press(move || send(TaskIntent::Retry(failed.get_untracked()))),
                )
                .child(
                    ui::library_action(
                        move || {
                            tr(if all_checked.get() {
                                "tasks.deselectAll"
                            } else {
                                "tasks.selectAll"
                            })
                        },
                        None,
                    )
                    .disabled(move || ids.get().is_empty())
                    .on_press(toggle_all),
                )
                .child(
                    ui::library_action(move || tr("tasks.selectFailed"), None)
                        .disabled(move || failed.get().is_empty())
                        .on_press(move || {
                            selected.set(failed.get_untracked().into_iter().collect())
                        }),
                )
                .child(
                    ui::library_action(move || tr("tasks.rescan"), Some(ui::Icon::ScanSearch))
                        .disabled(move || busy.get() || !capabilities.rescan)
                        .on_press(move || send(TaskIntent::Rescan)),
                ),
        );
    let viewport = signal(0.0_f32);
    let compact = computed(move || viewport.get() > 0.0 && viewport.get() < 708.0);
    let list_height = computed(move || {
        (items.get().len() as f32 * 53.0).min((window().height.0 - 182.0).max(53.0))
    });
    let table = move || {
        ui::task_table()
            .on_layout(move |bounds: &Bounds<Points>| {
                viewport.set((bounds.size.width.0 - 2.0).max(0.0))
            })
            .child(
                scroll()
                    .horizontal()
                    .test_id("task-table-horizontal")
                    .h(move || px(list_height.get() + 40.0))
                    .child(
                        view()
                            .w(move || {
                                px(viewport.get().max(ui::TaskColumn::minimum_table_width()))
                            })
                            .child(
                                ui::task_table_row(true)
                                    .child(
                                        ui::task_cell(ui::TaskColumn::Select, compact).child(
                                            ui::library_check(
                                                move || tr("tasks.selectAll"),
                                                all_checked,
                                                move || {
                                                    selected_count.get() > 0 && !all_checked.get()
                                                },
                                            )
                                            .on_change(move |_: &bool| toggle_all()),
                                        ),
                                    )
                                    .children(
                                        [
                                            (ui::TaskColumn::Title, "nav.tasks.title"),
                                            (ui::TaskColumn::Episode, "tasks.columns.episode"),
                                            (ui::TaskColumn::Status, "tasks.columns.status"),
                                            (ui::TaskColumn::Size, "tasks.columns.size"),
                                            (ui::TaskColumn::Actions, ""),
                                        ]
                                        .map(
                                            move |(column, key)| {
                                                ui::task_cell(column, compact).child(
                                                    text(move || {
                                                        if key.is_empty() {
                                                            SharedString::default()
                                                        } else {
                                                            tr(key)
                                                        }
                                                    })
                                                    .text_sm()
                                                    .line_height(px(20.0))
                                                    .font_weight(FontWeight::MEDIUM),
                                                )
                                            },
                                        ),
                                    ),
                            )
                            .child(
                                virtual_list(
                                    move || items.get().len(),
                                    move |at| {
                                        let item = computed(move || {
                                            items.with(|items| items.get(at.index()).cloned())
                                        });
                                        super::task_row::build(
                                            item,
                                            busy,
                                            capabilities,
                                            selected,
                                            compact,
                                            send,
                                        )
                                    },
                                )
                                .test_id("task-records")
                                .keys(move || {
                                    items
                                        .get()
                                        .into_iter()
                                        .map(|item| item.id)
                                        .collect::<Vec<_>>()
                                })
                                .estimated_item_size(px(53.0))
                                .h(move || px(list_height.get())),
                            ),
                    ),
            )
    };
    view()
        .flex_1()
        .p_6()
        .gap_4()
        .child(toolbar)
        .child(when(
            move || ids.get().is_empty(),
            move || ui::status(move || tr("tasks.empty")),
        ))
        .child(when(move || !ids.get().is_empty(), table))
        .child(when(
            move || selected_count.get() > 0,
            move || {
                ui::task_batch_bar()
                    .test_id("task-batch-actions")
                    .child(text(move || selected_count.get().to_string()).text_sm())
                    .child(when(
                        move || freeable.get() > 0,
                        move || {
                            text(move || {
                                tr("tasks.freeable")
                                    .replace("{size}", &crate::i18n::format_bytes(freeable.get()))
                            })
                            .text_xs()
                            .line_height(px(16.0))
                            .text_color(move || theme().muted_foreground)
                        },
                    ))
                    .child(view().flex_1())
                    .child(ui::task_batch_close().on_press(move || selected.set(HashSet::new())))
                    .child(
                        ui::batch_delete(move || tr("common.delete"))
                            .disabled(move || busy.get() || !capabilities.remove)
                            .on_press(move || {
                                send(TaskIntent::Remove(
                                    selected
                                        .get_untracked()
                                        .intersection(&ids.get_untracked())
                                        .cloned()
                                        .collect(),
                                ))
                            }),
                    )
            },
        ))
        .child(super::task_confirm::build(
            items,
            removing,
            confirmation,
            with_files,
            move |ids, files| {
                send(TaskIntent::Delete(ids, files));
            },
        ))
}
