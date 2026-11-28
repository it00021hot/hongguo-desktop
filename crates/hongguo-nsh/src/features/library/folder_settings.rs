use super::{
    folder_creation::{self, NameAction},
    folder_membership::Membership,
};
use crate::{i18n::tr, ui};
use hongguo_service::library_service::{self, BookshelfFolder};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(super) fn action(
    membership: Membership,
    folder: BookshelfFolder,
    saved: impl Fn(String, bool) + Copy + 'static,
    deleted: impl Fn() + Copy + 'static,
) -> View {
    let open = signal(false);
    let id = folder.id;
    let display_name = stored(folder.name.clone());
    let confirm_delete = signal(false);
    let owner = signal(None::<u64>);
    effect(move || {
        if let Some(opened_for) = owner.get()
            && (opened_for != membership.context.identity.get()
                || !membership.context.logged_in.get()
                || !membership.context.active.get())
        {
            batch(|| {
                owner.set(None);
                open.set(false);
                confirm_delete.set(false);
            });
        }
    });
    let close = move || {
        if !membership.busy.get_untracked() {
            open.set(false);
        }
    };
    let delete = move || {
        if !open.get_untracked()
            || owner.get_untracked() != Some(membership.context.identity.get_untracked())
            || !membership.context.active.get_untracked()
        {
            return;
        }
        let state = membership.session.with(|session| session.state.clone());
        membership.run(
            library_service::delete_collection_folder(state, id),
            move || {
                open.set(false);
                deleted();
            },
        );
    };
    let danger = move || {
        view()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(move || theme().border)
            .child(
                text(move || tr("collections.folderDanger"))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || theme().destructive),
            )
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        text(move || tr("collections.folderDeleteHint"))
                            .text_xs()
                            .text_color(move || theme().muted_foreground),
                    )
                    .child(
                        ui::controls::surface(ui::ButtonVariant::Quiet)
                            .accessibility_label(move || Some(tr("collections.deleteFolder")))
                            .disabled(membership.busy)
                            .text_color(move || theme().destructive)
                            .on_press(move || confirm_delete.set(true))
                            .child(text(move || tr("collections.deleteFolder")).text_sm()),
                    ),
            )
    };
    view()
        .child(
            ui::button(
                move || tr("collections.folderSettings"),
                ui::ButtonVariant::Secondary,
            )
            .disabled(membership.busy)
            .on_press(move || {
                membership.feedback.set(SharedString::default());
                confirm_delete.set(false);
                owner.set(Some(membership.context.identity.get_untracked()));
                open.set(true);
            }),
        )
        .child(if let Some(published) = folder.published {
            folder_creation::dialog(
                membership,
                open,
                confirm_delete,
                NameAction::Edit {
                    id,
                    published,
                    name: folder.name,
                },
                saved,
                danger,
            )
        } else {
            view().child(when(
                move || open.get() && !confirm_delete.get(),
                move || {
                    folder_creation::form_dialog(
                        "collections.folderSettings",
                        440.0,
                        membership.busy,
                        close,
                        view()
                            .gap_4()
                            .child(
                                text(move || tr("collections.folderSettings"))
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(ui::status(move || {
                                tr("collections.folderVisibilityUnavailable")
                            }))
                            .child(danger())
                            .child(ui::transient_notice(membership.feedback))
                            .child(
                                view().flex_row().justify_end().child(
                                    ui::button(
                                        move || tr("common.close"),
                                        ui::ButtonVariant::Secondary,
                                    )
                                    .disabled(membership.busy)
                                    .on_press(close),
                                ),
                            ),
                    )
                },
            ))
        })
        .child(ui::confirmation(
            confirm_delete,
            move || tr("collections.deleteFolderTitle"),
            move || tr("collections.confirmFolderDelete"),
            move || {
                text(move || {
                    tr("collections.deleteFolderBody").replace("{name}", &display_name.get())
                })
                .text_sm()
                .text_color(move || theme().muted_foreground)
            },
            delete,
        ))
}
