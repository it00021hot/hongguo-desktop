use super::{feed::Context, folder_card};
use crate::{bridge, events::SessionHandle, i18n::tr, ui};
use hongguo_service::library_service::BookshelfFolder;
use nsh_core::prelude::*;

type Feed = Resource<(u64, Vec<BookshelfFolder>), bridge::Failure>;

fn load(session: SessionHandle, context: Context) -> (Feed, Memo<Vec<BookshelfFolder>>) {
    let result = resource(
        move || {
            (context.logged_in.get() && context.active.get())
                .then(|| (context.identity.get(), context.revision.get()))
        },
        move |(identity, _), _| {
            let state = session.with(|session| session.state.clone());
            async move {
                bridge::request(hongguo_service::library_service::collection_folders(state))
                    .await
                    .map(|folders| (identity, folders))
            }
        },
    );
    let folders = computed(move || {
        let identity = context.identity.get();
        result.with(|result| {
            result
                .filter(|(owner, _)| *owner == identity)
                .map(|(_, folders)| folders.clone())
                .unwrap_or_default()
        })
    });
    (result, folders)
}

pub(super) fn index(
    session: SessionHandle,
    context: Context,
    busy: Signal<bool>,
    open: impl Fn(BookshelfFolder) + Copy + 'static,
) -> View {
    let (result, folders) = load(session, context);
    let empty = computed(move || folders.with(Vec::is_empty));

    view()
        .flex_1()
        .min_h_0()
        .gap_3()
        .child(when(
            move || result.loading() && folders.with(Vec::is_empty),
            move || super::loading::rows(2, if is_desktop() { 154.0 } else { 142.0 }),
        ))
        .child(when(
            move || result.error().is_some(),
            move || {
                crate::features::error_state(
                    move || {
                        result
                            .error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || result.refetch(),
                )
            },
        ))
        .child(when(
            move || {
                (!empty.get() || (!result.loading() && result.error().is_none()))
                    && result.with(|result| {
                        result.is_some_and(|(owner, _)| *owner == context.identity.get())
                    })
            },
            move || {
                view()
                    .gap_3()
                    .items_center()
                    .style_when(move || empty.get(), |style| style.flex_1().justify_center())
                    .style_when(
                        move || is_desktop() && !empty.get(),
                        |style| style.items_end(),
                    )
                    .child(when(empty, move || {
                        ui::status(move || tr("collections.officialFoldersEmpty"))
                    }))
                    .child(super::folder_creation::action(
                        session,
                        context,
                        move || {
                            result.refetch();
                        },
                    ))
            },
        ))
        .child(when(
            move || !empty.get(),
            move || gallery(folders, busy, open),
        ))
}

fn gallery(
    folders: Memo<Vec<BookshelfFolder>>,
    busy: Signal<bool>,
    open: impl Fn(BookshelfFolder) + Copy + 'static,
) -> View {
    let width = signal(0.0_f32);
    view()
        .flex_1()
        .min_h_0()
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .child(
            virtual_list(
                move || folders.with(Vec::len),
                move |at| {
                    let folder =
                        computed(move || folders.with(|folders| folders.get(at.index()).cloned()));
                    folder_card::card(folder, busy).on_press(move || {
                        if let Some(folder) = folder.get_untracked() {
                            open(folder);
                        }
                    })
                },
            )
            .flex_1()
            .columns(move || match width.get() {
                value if is_desktop() && value >= 1120.0 => 3u32,
                value if is_desktop() && value >= 680.0 => 2,
                _ => 1,
            })
            .column_gap(px(12.0))
            .row_gap(px(12.0))
            .keys(move || {
                folders.with(|folders| {
                    folders
                        .iter()
                        .map(|folder| folder.id.to_string())
                        .collect::<Vec<_>>()
                })
            })
            .estimated_item_size(if is_desktop() { px(154.0) } else { px(142.0) }),
        )
}
