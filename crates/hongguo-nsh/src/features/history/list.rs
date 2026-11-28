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
    now_ms: i64,
    events: HistoryEvents,
) -> View {
    let items = items.into_reactive();
    let items = computed(move || items.get());
    let events = stored(std::rc::Rc::new(events));
    let memory = crate::view_memory::scope("history");
    let tab = memory.signal("tab", HistoryTab::All);
    let query = memory.signal("query", String::new());
    let search_open = signal(false);
    let managing = signal(false);
    let selected = signal(HashSet::<String>::new());
    let confirm = signal(false);
    let removing = signal(Vec::<HistoryItem>::new());
    let busy = computed(move || events.with(|events| events.busy.get()));
    let width = signal(0.0_f32);
    let columns = computed(move || ui::poster_grid::columns(window().width.0));
    let card_height = computed(move || {
        ((width.get() - 32.0 - (columns.get() - 1) as f32 * 16.0) / columns.get() as f32).max(0.0)
            * 4.0
            / 3.0
            + 64.0
    });
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
        .min_h_0()
        .on_layout(move |frame: &Bounds<Points>| width.set(frame.size.width.0))
        .py_4()
        .gap_3()
        .style_when(move || !is_desktop(), |style| style.py_2().gap_2())
        .child(
            view()
                .px_4()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    scroll().horizontal().flex_1().min_w_0().child(view()
                        .flex_row()
                        .gap_2()
                        .children(HistoryTab::ALL.into_iter().map(move |value| {
                            crate::features::library::presentation::history_filter(move || value.label(), computed(move || tab.get() == value))
                                .min_h(ui::controls::control_height)
                                .h(ui::controls::control_height)
                                .on_press(move || tab.set(value))
                        }))),
                )
                .child(when(is_desktop, move || crate::features::library::presentation::library_search(query, move || tr("history.searchPlaceholder"))))
                .child(when(move || !is_desktop(), move || ui::controls::surface(ui::ButtonVariant::Quiet)
                    .accessibility_label(move || tr("history.searchPlaceholder"))
                    .size(px(48.0)).p_0().child(ui::icon(ui::Icon::Search))
                    .selected(search_open).on_press(move || search_open.update(|open| *open = !*open))))
                .child(
                    crate::features::library::presentation::library_action(
                        move || {
                            tr(if managing.get() {
                                "batch.exitManage"
                            } else {
                                "batch.manage"
                            })
                        },
                        None,
                    )
                    .style_when(move || !is_desktop(), |style| style.border(px(0.0)).bg(move || theme().background.opacity(0.0)))
                    .disabled(move || busy.get() || events.with(|events| events.remove.is_none()))
                    .on_press(move || {
                        if events.with(|events| events.remove.is_some()) {
                            managing.update(|value| *value = !*value);
                            selected.set(HashSet::new());
                        }
                    }),
                ),
        )
        .child(when(move || !is_desktop() && search_open.get(), move || view().px_4().child(
            crate::features::library::presentation::library_search(query, move || tr("history.searchPlaceholder")).w_full()
        )))
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
                            .w(move || px(((width.get() - 32.0 - (columns.get() - 1) as f32 * 16.0) / columns.get() as f32).max(0.0)))
                            .h(move || px(card_height.get()))
                            .overflow_hidden()
                            .pb(move || {
                                px(if at.index() + 1 == shown.with(Vec::len) {
                                    0.0
                                } else {
                                    8.0
                                })
                            })
                            .child(
                                crate::features::library::presentation::history_card()
                                    .style_when(
                                        move || item.get().is_none(),
                                        |style| style.hidden(),
                                    )
                                    .child(when(managing, move || {
                                        crate::features::library::presentation::personal_card_pick(
                                            move || {
                                                item.get()
                                                    .map(|item| SharedString::from(format!("{} · {}", item.title, time_ago(item.updated_at_ms, now_ms))))
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
                                        .disabled(busy)
                                        .on_change(move |_: &bool| open())
                                    }))
                                    .child(
                                        crate::features::library::presentation::history_open()
                                            .accessibility_label(move || {
                                                item.get()
                                                    .map(|item| SharedString::from(item.title))
                                            })
                                            .disabled(move || managing.get() || busy.get())
                                            .disabled_style(|style| style.opacity(move || if managing.get() { 1.0_f32 } else { 0.5 }))
                                            .on_press(move || { if !managing.get_untracked() { open(); } })
                                            .child(
                                                view().w_full().aspect_ratio(3.0 / 4.0).relative().overflow_hidden().rounded(px(8.0)).child(image(move || {
                                                    item.get()
                                                        .map(|item| item.cover)
                                                        .unwrap_or_else(|| {
                                                            nsh_core::element::ImageSrc::from("")
                                                        })
                                                })
                                                .absolute().inset_0()
                                                .flex_shrink_0()
                                                .rounded(px(8.0))
                                                .object_fit(nsh_core::ResizeMode::Cover)
                                                .bg(move || theme().muted)),
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
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .number_of_lines(1),
                                                    )
                                                    .child(
                                                        view()
                                                            .items_start()
                                                            .child(crate::features::library::presentation::history_progress(
                                                                move || {
                                                                    item.get()
                                                                        .map(|item| watching(&item))
                                                                        .unwrap_or_default()
                                                                },
                                                            )),
                                                    ),
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
            .flex_1()
            .columns(columns)
            .column_gap(px(16.0))
            .row_gap(px(16.0))
            .content_padding_x(px(16.0))
            .max_h(move || px(shown.with(Vec::len).div_ceil(columns.get() as usize) as f32 * (card_height.get() + 16.0)))
            .keys(move || {
                shown
                    .get()
                    .into_iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>()
            })
            .estimated_item_size(move || px(card_height.get())),
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
        .child(when(
            move || items.with(Vec::is_empty),
            crate::features::history::presentation::history_empty,
        ))
        .child(populated.style_when(move || items.with(Vec::is_empty), |style| style.hidden()))
}
