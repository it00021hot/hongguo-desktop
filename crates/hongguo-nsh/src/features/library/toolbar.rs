use super::model::Entry;
use super::{CollectionFolder, Kind, model::Query};
use crate::{i18n::tr, ui};
use nsh_base::theme;
use nsh_core::prelude::*;
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Copy)]
pub(super) struct Toolbar {
    pub kind: Kind,
    pub query: Signal<Query>,
    pub search: Signal<String>,
    pub category: Signal<i64>,
    pub(super) collection_folder: Memo<CollectionFolder>,
    pub online: Signal<bool>,
    pub busy: Signal<bool>,
    pub managing: Signal<bool>,
    pub selected: Signal<HashSet<String>>,
    pub counts: Memo<Option<(i64, i64)>>,
    pub logged_in: Memo<bool>,
}

pub(super) fn build(
    state: Toolbar,
    choose_folder: impl Fn(CollectionFolder) + Copy + 'static,
    leading: Option<View>,
) -> View {
    let manage = move || {
        crate::features::library::presentation::library_action(
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
            state.selected.update(HashSet::clear);
        })
    };
    let toolbar = view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_2();
    match state.kind {
        Kind::Follows => toolbar.children(leading).child(manage()),
        Kind::Collections => toolbar
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        crate::features::library::presentation::personal_tab(move || {
                            tr("collections.officialFolders")
                        })
                        .selected(move || {
                            matches!(
                                state.collection_folder.get(),
                                CollectionFolder::Index | CollectionFolder::Official(_)
                            )
                        })
                        .disabled(state.busy)
                        .on_press(move || choose_folder(CollectionFolder::Index)),
                    )
                    .children(
                        [
                            (0, "collections.filterAll"),
                            (1, "collections.filterReal"),
                            (1004, "collections.filterComic"),
                        ]
                        .into_iter()
                        .map(move |(value, key)| {
                            crate::features::library::presentation::personal_tab(move || tr(key))
                                .selected(move || {
                                    state.collection_folder.get() == CollectionFolder::All
                                        && state.category.get() == value
                                })
                                .disabled(state.busy)
                                .on_press(move || {
                                    if state.category.get_untracked() != value {
                                        state.category.set(value);
                                        state.selected.update(HashSet::clear);
                                    }
                                    choose_folder(CollectionFolder::All);
                                })
                        }),
                    ),
            )
            .child(when(
                move || state.collection_folder.get() == CollectionFolder::All,
                move || {
                    view()
                        .flex_row()
                        .flex_wrap()
                        .gap_2()
                        .child(crate::features::library::presentation::library_search(
                            state.search,
                            move || tr(state.kind.search()),
                        ))
                        .child(manage())
                },
            )),
        Kind::Liked => toolbar
            .justify_end()
            .child(crate::features::library::presentation::library_search(
                state.search,
                move || tr(state.kind.search()),
            ))
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
                        crate::features::library::presentation::personal_tab(move || {
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
                            state.selected.update(HashSet::clear);
                        })
                    }),
                ),
            )
            .child(crate::features::library::presentation::library_search(
                state.search,
                move || tr(state.kind.search()),
            ))
            .child(manage()),
    }
}

pub(super) fn batch(
    rows: super::rows::Rows,
    remove: impl Fn(Vec<Entry>) + Copy + 'static,
    extras: impl Fn() -> View + 'static,
) -> impl IntoElement {
    when(rows.managing, move || {
        let confirm = signal(false);
        let pending = signal(Vec::<Entry>::new());
        let scope = stored(rows.scroll_scope.get_untracked());
        let title = match rows.kind {
            Kind::Follows => "follow.batchCancelTitle",
            Kind::Collections => "collections.batchRemoveTitle",
            Kind::Liked => "liked.batchUndoTitle",
            Kind::Reservations => "reservation.batchCancelTitle",
        };
        let visible_keys = computed(move || {
            rows.items.with(|items| {
                items
                    .iter()
                    .map(|entry| entry.key.clone())
                    .collect::<HashSet<_>>()
            })
        });
        let selected_count = computed(move || {
            rows.selected
                .with(|keys| visible_keys.with(|visible| keys.intersection(visible).count()))
        });
        ui::batch_bar()
            .child(
                ui::batch_select(move || {
                    tr(
                        if !rows.items.with(Vec::is_empty)
                            && selected_count.get() == rows.items.with(Vec::len)
                        {
                            "batch.deselectAll"
                        } else {
                            "batch.selectAll"
                        },
                    )
                })
                .disabled(move || rows.busy.get() || rows.items.with(Vec::is_empty))
                .on_press(move || {
                    if selected_count.get_untracked() == rows.items.with(Vec::len) {
                        rows.selected.update(HashSet::clear);
                    } else {
                        rows.selected.set(visible_keys.get_untracked());
                    }
                }),
            )
            .child(
                text(move || {
                    tr("batch.selectedCount").replace("{count}", &selected_count.get().to_string())
                })
                .text_xs()
                .line_height(px(16.0))
                .text_color(move || theme().muted_foreground),
            )
            .child(view().flex_1())
            .child(extras())
            .child(
                ui::batch_delete(move || {
                    tr(if rows.busy.get() {
                        "batch.deleting"
                    } else {
                        rows.kind.action()
                    })
                })
                .disabled(move || rows.busy.get() || selected_count.get() == 0)
                .on_press(move || {
                    let entries = rows.selected.with(|keys| {
                        rows.items.with(|items| {
                            items
                                .iter()
                                .filter(|entry| keys.contains(&entry.key))
                                .map(|entry| Arc::as_ref(entry).clone())
                                .collect::<Vec<_>>()
                        })
                    });
                    if entries.is_empty() || rows.busy.get_untracked() {
                        return;
                    }
                    scope.replace(rows.scroll_scope.get_untracked());
                    pending.set(entries);
                    confirm.set(true);
                }),
            )
            .child(ui::confirmation(
                confirm,
                move || tr(title),
                move || {
                    tr(if rows.kind == Kind::Reservations {
                        "reservation.confirmCancel"
                    } else {
                        rows.kind.action()
                    })
                },
                move || {
                    text(move || {
                        tr(if rows.kind == Kind::Reservations {
                            "reservation.confirmCancelBody"
                        } else {
                            "batch.confirmBody"
                        })
                        .replace("{count}", &pending.with(Vec::len).to_string())
                    })
                    .text_sm()
                    .line_height(px(20.0))
                    .text_color(move || theme().muted_foreground)
                },
                move || {
                    if scope.with(|scope| *scope == rows.scroll_scope.get_untracked()) {
                        remove(pending.get_untracked());
                    }
                },
            ))
    })
}
