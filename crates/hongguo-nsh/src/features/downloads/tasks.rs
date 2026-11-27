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

pub(super) fn content(
    items: impl IntoReactive<Vec<DownloadTask>>,
    status: impl IntoReactive<QueueStatus>,
    busy: impl IntoReactive<bool>,
    dispatch: impl Fn(TaskIntent) + 'static,
    open_browse: impl Fn() + Copy + 'static,
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
        if !busy.get_untracked() {
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
        .style_when(
            move || !is_desktop() && ids.get().is_empty(),
            |style| style.hidden(),
        )
        .flex_shrink_0()
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
                    crate::features::library::presentation::library_action(
                        move || tr("tasks.startAll"),
                        Some(ui::Icon::Play),
                    )
                    .disabled(busy)
                    .on_press(move || send(TaskIntent::Resume)),
                )
                .child(
                    crate::features::library::presentation::library_action(
                        move || tr("tasks.pauseAll"),
                        Some(ui::Icon::Pause),
                    )
                    .disabled(busy)
                    .on_press(move || send(TaskIntent::Pause)),
                )
                .child(
                    crate::features::library::presentation::library_action(
                        move || tr("tasks.retryAll"),
                        Some(ui::Icon::Retry),
                    )
                    .disabled(move || busy.get() || failed.get().is_empty())
                    .on_press(move || send(TaskIntent::Retry(failed.get_untracked()))),
                )
                .child(
                    crate::features::library::presentation::library_action(
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
                    crate::features::library::presentation::library_action(
                        move || tr("tasks.selectFailed"),
                        None,
                    )
                    .disabled(move || failed.get().is_empty())
                    .on_press(move || selected.set(failed.get_untracked().into_iter().collect())),
                )
                .child(
                    crate::features::library::presentation::library_action(
                        move || tr("tasks.rescan"),
                        Some(ui::Icon::ScanSearch),
                    )
                    .disabled(busy)
                    .on_press(move || send(TaskIntent::Rescan)),
                ),
        );
    let table_viewport = signal((0.0_f32, 0.0_f32));
    let compact = computed(move || {
        let (width, _) = table_viewport.get();
        width > 0.0 && width < 708.0
    });
    let table = move || {
        crate::features::library::presentation::task_table()
            .flex_1()
            .min_h_0()
            .on_layout(move |bounds: &Bounds<Points>| {
                table_viewport.set((
                    (bounds.size.width.0 - 2.0).max(0.0),
                    (bounds.size.height.0 - 2.0).max(0.0),
                ));
            })
            .child(
                scroll()
                    .horizontal()
                    .flex_1()
                    .min_h_0()
                    .child(
                        view()
                            .w(move || {
                                px(table_viewport.get().0.max(crate::features::library::presentation::TaskColumn::minimum_table_width()))
                            })
                            .h(move || px(table_viewport.get().1))
                            .child(
                                crate::features::library::presentation::task_table_row(true)
                                    .flex_shrink_0()
                                    .child(
                                        crate::features::library::presentation::task_cell(crate::features::library::presentation::TaskColumn::Select, compact).child(
                                            crate::features::library::presentation::library_check(
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
                                            (crate::features::library::presentation::TaskColumn::Title, "nav.tasks.title"),
                                            (crate::features::library::presentation::TaskColumn::Episode, "tasks.columns.episode"),
                                            (crate::features::library::presentation::TaskColumn::Status, "tasks.columns.status"),
                                            (crate::features::library::presentation::TaskColumn::Size, "tasks.columns.size"),
                                            (crate::features::library::presentation::TaskColumn::Actions, ""),
                                        ]
                                        .map(
                                            move |(column, key)| {
                                                crate::features::library::presentation::task_cell(column, compact).child(
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
                                        super::task_row::build(item, busy, selected, compact, send)
                                    },
                                )
                                .keys(move || {
                                    items
                                        .get()
                                        .into_iter()
                                        .map(|item| item.id)
                                        .collect::<Vec<_>>()
                                })
                                .estimated_item_size(px(53.0))
                                .flex_1()
                                .min_h_0(),
                            ),
                    ),
            )
    };
    view()
        .flex_1()
        .min_h_0()
        .p_6()
        .gap_4()
        .child(toolbar)
        .child(
            view()
                .flex_shrink_0()
                .flex_row()
                .items_start()
                .gap_2()
                .px_3()
                .py_2()
                .rounded(ui::radii::control)
                .bg(move || theme().muted.opacity(0.55))
                .text_color(move || theme().muted_foreground)
                .child(
                    ui::icon(ui::Icon::MonitorPlay)
                        .size(px(15.0))
                        .mt(px(1.0))
                        .flex_shrink_0()
                        .accessible(false),
                )
                .child(
                    text(move || tr("tasks.bytevc2PlaybackNote"))
                        .text_xs()
                        .line_height(px(18.0)),
                ),
        )
        .child(when(
            move || ids.get().is_empty(),
            move || {
                if is_desktop() {
                    return ui::status(move || tr("tasks.empty"));
                }
                view()
                    .py_16()
                    .items_center()
                    .gap_3()
                    .text_color(move || theme().muted_foreground)
                    .child(ui::icon(ui::Icon::Download).size_8())
                    .child(
                        text(move || tr("tasks.empty"))
                            .text_base()
                            .text_color(move || theme().foreground),
                    )
                    .child(text(move || tr("tasks.emptyHint")).text_sm())
                    .child(
                        ui::button(move || tr("nav.browse.title"), ui::ButtonVariant::Primary)
                            .on_press(open_browse),
                    )
                    .child(
                        ui::button(move || tr("tasks.rescan"), ui::ButtonVariant::Quiet)
                            .disabled(busy)
                            .on_press(move || send(TaskIntent::Rescan)),
                    )
            },
        ))
        .child(when(move || !ids.get().is_empty(), table))
        .child(when(
            move || selected_count.get() > 0,
            move || {
                crate::features::library::presentation::task_batch_bar()
                    .flex_shrink_0()
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
                    .child(
                        crate::features::library::presentation::task_batch_close()
                            .on_press(move || selected.set(HashSet::new())),
                    )
                    .child(
                        ui::batch_delete(move || tr("common.delete"))
                            .disabled(busy)
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
