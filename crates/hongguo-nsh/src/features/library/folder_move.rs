use super::{folder_membership::Membership, model::Entry};
use crate::{bridge, i18n::tr, ui};
use hongguo_service::library_service;
use nsh_core::prelude::*;

pub(super) fn action(
    membership: Membership,
    source: i64,
    entries: Memo<Vec<Entry>>,
    done: impl Fn() + Copy + 'static,
) -> View {
    let open = signal(false);
    let target = signal(None::<i64>);
    let destinations = resource(
        move || open.get().then(|| membership.context.identity.get()),
        move |identity, _| {
            let state = membership.session.with(|session| session.state.clone());
            async move {
                bridge::request(library_service::collection_folders(state))
                    .await
                    .map(|mut folders| {
                        folders.retain(|folder| folder.id != source);
                        (identity, folders)
                    })
            }
        },
    );
    let folders = computed(move || {
        destinations.with(|response| {
            response
                .filter(|(owner, _)| *owner == membership.context.identity.get())
                .map_or_else(Vec::new, |(_, folders)| folders.clone())
        })
    });
    let target_valid = computed(move || {
        target
            .get()
            .is_some_and(|id| folders.with(|folders| folders.iter().any(|folder| folder.id == id)))
    });
    let close = move || {
        if !membership.busy.get_untracked() {
            open.set(false);
        }
    };
    let submit = move || {
        if !target_valid.get_untracked() || membership.busy.get_untracked() {
            return;
        }
        let Some(destination) = target.get_untracked() else {
            return;
        };
        let entries = entries.get_untracked();
        if entries.is_empty() {
            return;
        }
        let state = membership.session.with(|session| session.state.clone());
        membership.run(
            library_service::move_collection_items(state, source, destination, entries),
            move || {
                open.set(false);
                done();
            },
        );
    };
    view()
        .child(
            ui::controls::compact_button(
                move || tr("collections.moveFolderItems"),
                ui::ButtonVariant::Secondary,
            )
            .disabled(move || membership.busy.get() || entries.with(Vec::is_empty))
            .on_press(move || {
                target.set(None);
                membership.feedback.set(SharedString::default());
                open.set(true);
            }),
        )
        .child(when(open, move || {
            super::folder_creation::form_dialog(
                "collections.moveFolderTitle",
                440.0,
                membership.busy,
                close,
                view()
                    .gap_3()
                    .min_h_0()
                    .child(
                        text(move || tr("collections.moveFolderTitle"))
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(when(
                        move || destinations.loading() && folders.with(Vec::is_empty),
                        move || ui::loading::indicator("common.loading").min_h(px(56.0)),
                    ))
                    .child(when(
                        move || destinations.error().is_some(),
                        move || {
                            crate::features::error_state(
                                move || {
                                    destinations
                                        .error()
                                        .map_or_else(SharedString::default, |error| error.text())
                                },
                                move || destinations.refetch(),
                            )
                        },
                    ))
                    .child(when(
                        move || {
                            !destinations.loading()
                                && destinations.error().is_none()
                                && folders.with(Vec::is_empty)
                        },
                        move || ui::status(move || tr("collections.noMoveDestination")),
                    ))
                    .child(
                        virtual_list(
                            move || folders.with(Vec::len),
                            move |at| {
                                let item = computed(move || {
                                    folders.with(|folders| folders.get(at.index()).cloned())
                                });
                                ui::button(
                                    move || {
                                        item.with(|item| {
                                            item.as_ref()
                                                .map_or_else(SharedString::default, |item| {
                                                    item.name.clone().into()
                                                })
                                        })
                                    },
                                    ui::ButtonVariant::Secondary,
                                )
                                .selected(move || {
                                    item.with(|item| {
                                        item.as_ref()
                                            .is_some_and(|item| target.get() == Some(item.id))
                                    })
                                })
                                .disabled(membership.busy)
                                .on_press(move || {
                                    target.set(item.with(|item| item.as_ref().map(|item| item.id)))
                                })
                            },
                        )
                        .h(move || {
                            let count = folders.with(Vec::len);
                            let height = count as f32 * 42.0 + count.saturating_sub(1) as f32 * 6.0;
                            px(height.min((window().height.0 * 0.5).min(288.0)))
                        })
                        .row_gap(px(6.0))
                        .estimated_item_size(px(42.0))
                        .keys(move || {
                            folders.with(|folders| {
                                folders
                                    .iter()
                                    .map(|folder| folder.id.to_string())
                                    .collect::<Vec<_>>()
                            })
                        }),
                    )
                    .child(ui::status(membership.feedback))
                    .child(
                        view()
                            .flex_row()
                            .justify_end()
                            .gap_2()
                            .child(
                                ui::button(
                                    move || tr("common.cancel"),
                                    ui::ButtonVariant::Secondary,
                                )
                                .disabled(membership.busy)
                                .on_press(close),
                            )
                            .child(
                                ui::button(
                                    move || tr("collections.confirmFolderMove"),
                                    ui::ButtonVariant::Primary,
                                )
                                .disabled(move || membership.busy.get() || !target_valid.get())
                                .on_press(submit),
                            ),
                    ),
            )
        }))
}
