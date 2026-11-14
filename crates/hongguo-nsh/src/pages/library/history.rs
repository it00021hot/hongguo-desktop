use super::model::{HistoryItem, HistoryTab, time_ago, watching};
use crate::{i18n::tr, ui};
use nsh_base::theme;
use nsh_core::prelude::*;
use std::collections::HashSet;

pub(super) struct HistoryEvents {
    pub open: Box<dyn Fn(&HistoryItem)>,
    pub remove: Option<Box<dyn Fn(Vec<HistoryItem>)>>,
    pub busy: Memo<bool>,
    pub reset: Memo<(u64, u64)>,
}

pub(super) fn content(
    items: impl IntoReactive<Vec<HistoryItem>>,
    local_mode: impl IntoReactive<bool>,
    now_ms: i64,
    events: HistoryEvents,
) -> View {
    let items = items.into_reactive();
    let items = computed(move || items.get());
    let events = stored(std::rc::Rc::new(events));
    let memory = crate::view_memory::scope("history");
    let tab = memory.signal("tab", HistoryTab::All);
    let query = memory.signal("query", String::new());
    let managing = signal(false);
    let selected = signal(HashSet::<String>::new());
    let confirm = signal(false);
    let removing = signal(Vec::<HistoryItem>::new());
    let busy = computed(move || events.with(|events| events.busy.get()));
    effect(move || {
        events.with(|events| events.reset.get());
        managing.set(false);
        selected.set(HashSet::new());
        confirm.set(false);
    });
    let shown = computed(move || {
        items
            .get()
            .into_iter()
            .filter(|item| tab.get().matches(item, &query.get()))
            .collect::<Vec<_>>()
    });
    let populated = view()
        .flex_1()
        .py_4()
        .gap_3()
        .child(when(local_mode, move || {
            text(move || tr("history.localModeHint"))
                .px_5()
                .text_xs()
                .line_height(px(16.0))
                .text_color(move || theme().muted_foreground)
        }))
        .child(
            view()
                .px_4()
                .flex_row()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    view()
                        .flex_row()
                        .gap_2()
                        .children(HistoryTab::ALL.into_iter().map(move |value| {
                            ui::history_filter(move || value.label())
                                .selected(move || tab.get() == value)
                                .on_press(move || tab.set(value))
                        })),
                )
                .child(ui::library_search(query, move || {
                    tr("history.searchPlaceholder")
                }))
                .child(
                    ui::library_action(
                        move || {
                            tr(if managing.get() {
                                "batch.exitManage"
                            } else {
                                "batch.manage"
                            })
                        },
                        None,
                    )
                    .disabled(move || busy.get() || events.with(|events| events.remove.is_none()))
                    .on_press(move || {
                        if events.with(|events| events.remove.is_some()) {
                            managing.update(|value| *value = !*value);
                            selected.set(HashSet::new());
                        }
                    }),
                ),
        )
        .child(when(
            move || shown.get().is_empty(),
            move || {
                view()
                    .py_16()
                    .items_center()
                    .gap_2()
                    .child(ui::icon(ui::Icon::History).size(px(32.0)).opacity(0.4))
                    .child(
                        text(move || {
                            tr(if !query.get().trim().is_empty() {
                                "common.searchNoMatch"
                            } else if tab.get() == HistoryTab::Finished {
                                "history.noMatchFinished"
                            } else {
                                "history.noMatchUnfinished"
                            })
                        })
                        .text_sm()
                        .text_color(move || theme().muted_foreground),
                    )
            },
        ))
        .child(
            crate::view_memory::remember_scroll(
                virtual_list(
                    move || shown.get().len(),
                    move |at| {
                        let item =
                            computed(move || shown.with(|items| items.get(at.index()).cloned()));
                        let open = move || {
                            let Some(current) = item.get_untracked() else {
                                return;
                            };
                            if busy.get_untracked() {
                                return;
                            }
                            if managing.get_untracked() {
                                selected.update(|selected| {
                                    let id = current.id;
                                    if !selected.remove(&id) {
                                        selected.insert(id);
                                    }
                                });
                            } else {
                                let events = events.with(|events| events.clone());
                                (events.open)(&current);
                            }
                        };
                        view()
                            .pb(move || {
                                px(if at.index() + 1 == shown.with(Vec::len) {
                                    0.0
                                } else {
                                    8.0
                                })
                            })
                            .child(
                                ui::history_card()
                                    .style_when(
                                        move || item.get().is_none(),
                                        |style| style.hidden(),
                                    )
                                    .child(when(managing, move || {
                                        ui::personal_pick(
                                            move || {
                                                item.get()
                                                    .map(|item| SharedString::from(item.title))
                                                    .unwrap_or_default()
                                            },
                                            move || {
                                                item.get().is_some_and(|item| {
                                                    selected.with(|selected| {
                                                        selected.contains(&item.id)
                                                    })
                                                })
                                            },
                                        )
                                        .on_change(move |_: &bool| open())
                                    }))
                                    .child(
                                        ui::history_open()
                                            .accessibility_label(move || {
                                                item.get()
                                                    .map(|item| SharedString::from(item.title))
                                            })
                                            .on_press(open)
                                            .child(
                                                image(move || {
                                                    item.get()
                                                        .map(|item| item.cover)
                                                        .unwrap_or_else(|| {
                                                            nsh_core::element::ImageSrc::from("")
                                                        })
                                                })
                                                .w(px(72.0))
                                                .h(px(96.0))
                                                .flex_shrink_0()
                                                .rounded(px(8.0))
                                                .object_fit(nsh_core::ResizeMode::Cover)
                                                .bg(move || theme().muted),
                                            )
                                            .child(
                                                view()
                                                    .flex_1()
                                                    .min_w(px(0.0))
                                                    .gap_1p5()
                                                    .child(
                                                        text(move || {
                                                            let title = item
                                                                .get()
                                                                .map(|item| item.title)
                                                                .unwrap_or_default();
                                                            if title.is_empty() {
                                                                tr("history.unknownTitle")
                                                                    .to_string()
                                                            } else {
                                                                title
                                                            }
                                                        })
                                                        .text_sm()
                                                        .line_height(px(20.0))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .number_of_lines(1),
                                                    )
                                                    .child(
                                                        view()
                                                            .flex_row()
                                                            .flex_wrap()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(ui::history_progress(
                                                                move || {
                                                                    item.get()
                                                                        .map(|item| watching(&item))
                                                                        .unwrap_or_default()
                                                                },
                                                            ))
                                                            .child(
                                                                text(move || {
                                                                    item.get()
                                                                        .map(|item| {
                                                                            time_ago(
                                                                                item.updated_at_ms,
                                                                                now_ms,
                                                                            )
                                                                        })
                                                                        .unwrap_or_default()
                                                                })
                                                                .text_xs()
                                                                .line_height(px(16.0))
                                                                .text_color(move || {
                                                                    theme().muted_foreground
                                                                }),
                                                            ),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        ui::library_action(
                                            move || tr("history.continue"),
                                            Some(ui::Icon::Play),
                                        )
                                        .disabled(busy)
                                        .on_press(
                                            move || {
                                                if let Some(item) = item.get_untracked() {
                                                    let events =
                                                        events.with(|events| events.clone());
                                                    (events.open)(&item);
                                                }
                                            },
                                        ),
                                    ),
                            )
                            .into_element()
                    },
                ),
                "history",
                computed(move || {
                    (
                        tab.get(),
                        query.get(),
                        events.with(|events| events.reset.get().0),
                    )
                }),
            )
            .test_id("history-records")
            .flex_1()
            .content_padding_x(px(16.0))
            .max_h(move || px((shown.with(Vec::len) as f32 * 130.0 - 8.0).max(0.0)))
            .keys(move || {
                shown
                    .get()
                    .into_iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>()
            })
            .estimated_item_size(px(130.0)),
        )
        .child(when(managing, move || {
            let selected_items = move || {
                items
                    .get()
                    .into_iter()
                    .filter(|item| selected.with(|ids| ids.contains(&item.id)))
                    .collect::<Vec<_>>()
            };
            ui::batch_bar()
                .mx_4()
                .child(
                    ui::batch_select(move || {
                        tr(
                            if !shown.get().is_empty()
                                && selected_items().len() == shown.with(Vec::len)
                            {
                                "batch.deselectAll"
                            } else {
                                "batch.selectAll"
                            },
                        )
                    })
                    .disabled(busy)
                    .on_press(move || {
                        selected.set(if selected_items().len() == shown.with(Vec::len) {
                            HashSet::new()
                        } else {
                            shown
                                .get_untracked()
                                .into_iter()
                                .map(|item| item.id)
                                .collect()
                        });
                    }),
                )
                .child(
                    text(move || {
                        tr("batch.selectedCount")
                            .replace("{count}", &selected_items().len().to_string())
                    })
                    .text_xs(),
                )
                .child(view().flex_1())
                .child(
                    ui::batch_delete(move || {
                        tr(if busy.get() {
                            "batch.deleting"
                        } else {
                            "batch.deleteSelected"
                        })
                    })
                    .disabled(move || busy.get() || selected_items().is_empty())
                    .on_press(move || {
                        removing.set(selected_items());
                        confirm.set(true);
                    }),
                )
        }))
        .child(ui::confirmation(
            confirm,
            move || tr("history.deleteConfirmTitle"),
            move || tr("common.confirm"),
            move || {
                text(move || {
                    tr("batch.confirmBody").replace("{count}", &removing.with(Vec::len).to_string())
                })
                .text_sm()
                .text_color(move || theme().muted_foreground)
            },
            move || {
                let events = events.with(|events| events.clone());
                if let Some(remove) = &events.remove {
                    remove(removing.get_untracked());
                }
            },
        ));
    view()
        .flex_1()
        .child(when(move || items.with(Vec::is_empty), ui::history_empty))
        .child(populated.style_when(move || items.with(Vec::is_empty), |style| style.hidden()))
}
