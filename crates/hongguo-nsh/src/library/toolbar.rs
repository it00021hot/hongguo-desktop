use super::model::Entry;
use super::{Kind, model::Query};
use crate::{i18n::tr, ui};
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct Toolbar {
    pub kind: Kind,
    pub query: Signal<Query>,
    pub search: Signal<String>,
    pub category: Signal<i64>,
    pub online: Signal<bool>,
    pub busy: Signal<bool>,
    pub managing: Signal<bool>,
    pub selected: Signal<Vec<String>>,
    pub counts: Memo<Option<(i64, i64)>>,
    pub logged_in: Memo<bool>,
}

pub(super) fn build(state: Toolbar) -> View {
    let manage = move || {
        ui::library_action(
            move || {
                tr(if state.managing.get() {
                    "batch.exitManage"
                } else {
                    "batch.manage"
                })
            },
            None,
        )
        .disabled(state.busy)
        .style_when(move || !state.logged_in.get(), |style| style.hidden())
        .on_press(move || {
            state.managing.update(|value| *value = !*value);
            state.selected.set(Vec::new());
        })
    };
    let toolbar = view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_2();
    match state.kind {
        Kind::Follows => view().flex_row().child(manage()),
        Kind::Collections => toolbar
            .child(
                view().flex_row().items_center().gap_2().children(
                    [
                        (0, "collections.filterAll"),
                        (1, "collections.filterReal"),
                        (1004, "collections.filterComic"),
                    ]
                    .into_iter()
                    .map(move |(value, key)| {
                        ui::history_filter(move || tr(key))
                            .selected(move || state.category.get() == value)
                            .on_press(move || state.category.set(value))
                    }),
                ),
            )
            .child(ui::library_search(state.search, move || {
                tr(state.kind.search())
            }))
            .child(manage()),
        Kind::Liked => toolbar
            .justify_end()
            .child(ui::library_search(state.search, move || {
                tr(state.kind.search())
            }))
            .child(manage()),
        Kind::Reservations => toolbar
            .child(
                view().flex_row().gap_2().children(
                    [
                        (true, "reservation.tab.online"),
                        (false, "reservation.tab.offline"),
                    ]
                    .into_iter()
                    .map(move |(online, key)| {
                        ui::personal_tab(move || {
                            let count = state
                                .counts
                                .get()
                                .map(|(yes, no)| if online { yes } else { no });
                            match count {
                                Some(count) => format!("{}  {count}", tr(key)).into(),
                                None => tr(key),
                            }
                        })
                        .selected(move || state.online.get() == online)
                        .disabled(state.busy)
                        .on_press(move || {
                            state.online.set(online);
                            state.query.set(Query {
                                online,
                                ..Default::default()
                            });
                            state.selected.set(Vec::new());
                        })
                    }),
                ),
            )
            .child(ui::library_search(state.search, move || {
                tr(state.kind.search())
            }))
            .child(manage()),
    }
}

pub(super) fn batch(
    rows: super::rows::Rows,
    remove: impl Fn(Vec<Entry>) + Copy + 'static,
    report: impl Fn(SharedString) + Copy + 'static,
) -> impl IntoElement {
    when(rows.managing, move || {
        let selected_entries = move || {
            rows.items
                .get()
                .into_iter()
                .filter(|entry| rows.selected.with(|keys| keys.contains(&entry.key)))
                .collect::<Vec<_>>()
        };
        ui::batch_bar()
            .child(
                ui::batch_select(move || {
                    tr(
                        if rows.items.with(Vec::len) > 0
                            && rows.selected.with(Vec::len) >= rows.items.with(Vec::len)
                        {
                            "batch.deselectAll"
                        } else {
                            "batch.selectAll"
                        },
                    )
                })
                .disabled(move || rows.busy.get() || rows.items.with(Vec::is_empty))
                .on_press(move || {
                    if rows.selected.with(Vec::len) >= rows.items.with(Vec::len) {
                        rows.selected.set(Vec::new());
                    } else {
                        rows.selected.set(
                            rows.items
                                .get_untracked()
                                .into_iter()
                                .map(|entry| entry.key)
                                .collect(),
                        );
                    }
                }),
            )
            .child(
                text(move || {
                    tr("batch.selectedCount")
                        .replace("{count}", &rows.selected.with(Vec::len).to_string())
                })
                .text_xs()
                .line_height(px(16.0))
                .text_color(move || theme().muted_foreground),
            )
            .child(view().flex_1())
            .child(
                ui::batch_delete(move || {
                    tr(if rows.busy.get() {
                        "batch.deleting"
                    } else {
                        rows.kind.action()
                    })
                })
                .disabled(move || rows.busy.get() || selected_entries().is_empty())
                .on_press(move || {
                    let entries = selected_entries();
                    if entries.is_empty() || rows.busy.get_untracked() {
                        return;
                    }
                    let title = match rows.kind {
                        Kind::Follows => "follow.batchCancelTitle",
                        Kind::Collections => "collections.batchRemoveTitle",
                        Kind::Liked => "liked.batchUndoTitle",
                        Kind::Reservations => "reservation.batchCancelTitle",
                    };
                    let confirmation = nsh_base::alert_dialog()
                        .title(tr(title))
                        .description(
                            tr("batch.confirmBody").replace("{count}", &entries.len().to_string()),
                        )
                        .cancel("cancel", tr("common.cancel"))
                        .destructive("remove", tr("common.confirm"))
                        .show();
                    spawn(async move {
                        match confirmation.await {
                            Ok(answer) if answer.action.as_deref() == Some("remove") => {
                                remove(entries)
                            }
                            Ok(_) => {}
                            Err(error) => report(error.into()),
                        }
                    });
                }),
            )
    })
}
