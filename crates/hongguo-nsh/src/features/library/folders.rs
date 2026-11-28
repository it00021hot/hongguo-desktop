use super::{feed::Context, folder_card};
use crate::{bridge, events::SessionHandle, i18n::tr, ui};
use hongguo_service::library_service::{BookshelfFolder, BookshelfFolderPage};
use nsh_core::prelude::*;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq)]
struct Scope {
    identity: u64,
    revision: u64,
    refresh: u64,
}

#[derive(Clone, Copy, PartialEq)]
struct Request {
    scope: Scope,
    offset: i64,
}

#[derive(Clone, Default, PartialEq)]
struct Snapshot {
    scope: Option<Scope>,
    items: Vec<BookshelfFolder>,
    seen: HashSet<i64>,
    next: Option<i64>,
}

impl Snapshot {
    fn apply(&mut self, request: Request, page: BookshelfFolderPage<BookshelfFolder>) {
        if request.offset == 0 {
            *self = Self {
                scope: Some(request.scope),
                ..Self::default()
            };
        } else if self.scope != Some(request.scope) || self.next != Some(request.offset) {
            return;
        }
        self.items.extend(
            page.items
                .into_iter()
                .filter(|folder| self.seen.insert(folder.id)),
        );
        self.next = page.next_offset;
    }
}

type Feed = Resource<(Request, BookshelfFolderPage<BookshelfFolder>), bridge::Failure>;

#[derive(Clone, Copy)]
pub(super) struct Folders {
    source: Memo<Option<Request>>,
    request: Signal<Option<Request>>,
    refresh: Signal<u64>,
    result: Feed,
    snapshot: Signal<Snapshot>,
}

impl Folders {
    pub(super) fn load(
        session: SessionHandle,
        context: Context,
        visible: impl Fn() -> bool + 'static,
    ) -> Self {
        let request = signal(None::<Request>);
        let refresh = signal(0_u64);
        let snapshot = signal(Snapshot::default());
        let source = computed(move || {
            (context.logged_in.get() && context.active.get() && visible()).then(|| {
                let scope = Scope {
                    identity: context.identity.get(),
                    revision: context.revision.get(),
                    refresh: refresh.get(),
                };
                request
                    .get()
                    .filter(|request| request.scope == scope)
                    .unwrap_or(Request { scope, offset: 0 })
            })
        });
        let result = resource(
            move || source.get(),
            move |request, _| {
                let state = session.with(|session| session.state.clone());
                async move {
                    bridge::request(hongguo_service::library_service::collection_folder_page(
                        state,
                        request.offset,
                    ))
                    .await
                    .map(|page| (request, page))
                }
            },
        );
        effect(move || {
            if !result.loading()
                && let Some((request, page)) = result.get()
                && source.get_untracked() == Some(request)
            {
                snapshot.update(|value| value.apply(request, page));
            }
        });
        Self {
            source,
            request,
            refresh,
            result,
            snapshot,
        }
    }

    pub(super) fn loading(self) -> bool {
        self.result.loading()
    }

    pub(super) fn error(self) -> Option<bridge::Failure> {
        self.result.error()
    }

    pub(super) fn retry(self) {
        self.result.refetch();
    }

    pub(super) fn items(self, identity: u64, exclude: Option<i64>) -> Vec<BookshelfFolder> {
        self.snapshot.with(|snapshot| {
            if snapshot
                .scope
                .is_some_and(|scope| scope.identity == identity)
            {
                snapshot
                    .items
                    .iter()
                    .filter(|folder| exclude != Some(folder.id))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            }
        })
    }

    pub(super) fn has_more(self) -> bool {
        self.source.get().is_some_and(|request| {
            self.snapshot
                .with(|snapshot| snapshot.scope == Some(request.scope) && snapshot.next.is_some())
        })
    }

    pub(super) fn advance(self) {
        if self.result.loading() || self.result.error().is_some() {
            return;
        }
        let Some(current) = self.source.get_untracked() else {
            return;
        };
        let next = self.snapshot.with_untracked(|snapshot| {
            (snapshot.scope == Some(current.scope))
                .then_some(snapshot.next)
                .flatten()
        });
        if let Some(offset) = next.filter(|offset| *offset > current.offset) {
            self.request.set(Some(Request {
                scope: current.scope,
                offset,
            }));
        }
    }
}

pub(super) fn index(
    session: SessionHandle,
    context: Context,
    busy: Signal<bool>,
    open: impl Fn(BookshelfFolder) + Copy + 'static,
) -> View {
    let model = crate::view_memory::cached("collection-folders", move || {
        Folders::load(session, context, || true)
    });
    let result = model.result;
    let folders = computed(move || model.items(context.identity.get(), None));
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
                    && model.snapshot.with(|snapshot| {
                        snapshot
                            .scope
                            .is_some_and(|scope| scope.identity == context.identity.get())
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
                            model.refresh.update(|value| *value = value.wrapping_add(1));
                        },
                    ))
            },
        ))
        .child(when(
            move || !empty.get(),
            move || gallery(folders, busy, open, move || model.advance()),
        ))
        .child(when(
            move || model.has_more(),
            move || {
                ui::button(
                    move || {
                        tr(if result.loading() {
                            "feed.loadingMore"
                        } else {
                            "follow.loadMore"
                        })
                    },
                    ui::ButtonVariant::Secondary,
                )
                .self_end()
                .disabled(move || result.loading() || busy.get() || result.error().is_some())
                .on_press(move || model.advance())
            },
        ))
}

fn gallery(
    folders: Memo<Vec<BookshelfFolder>>,
    busy: Signal<bool>,
    open: impl Fn(BookshelfFolder) + Copy + 'static,
    load_more: impl Fn() + 'static,
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
            .on_load_more(load_more)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn page(offset: i64, total: i64) -> BookshelfFolderPage<BookshelfFolder> {
        let end = (offset + 20).min(total);
        BookshelfFolderPage {
            items: (offset..end)
                .map(|id| BookshelfFolder {
                    id,
                    name: format!("fixture {id}"),
                    item_count: None,
                    published: None,
                    covers: Vec::new(),
                })
                .collect(),
            next_offset: (end < total).then_some(end),
            folder: None,
        }
    }

    #[test]
    fn folder_index_shows_first_page_and_merges_all_continuations() {
        let scope = Scope {
            identity: 1,
            revision: 1,
            refresh: 0,
        };
        for total in [0, 1, 1_000, 5_000] {
            let mut snapshot = Snapshot::default();
            snapshot.apply(Request { scope, offset: 0 }, page(0, total));
            assert_eq!(snapshot.items.len(), total.min(20) as usize);
            while let Some(offset) = snapshot.next {
                snapshot.apply(Request { scope, offset }, page(offset, total));
            }
            assert!(snapshot.items.iter().map(|item| item.id).eq(0..total));
        }
    }

    #[test]
    fn folder_index_deduplicates_overlap_and_ignores_old_account_continuation() {
        let original = Scope {
            identity: 1,
            revision: 1,
            refresh: 0,
        };
        let edited = Scope {
            identity: 2,
            ..original
        };
        let mut snapshot = Snapshot::default();
        snapshot.apply(
            Request {
                scope: original,
                offset: 0,
            },
            page(0, 50),
        );
        snapshot.apply(
            Request {
                scope: original,
                offset: 20,
            },
            page(19, 50),
        );
        assert_eq!(snapshot.items.len(), 39);
        snapshot.apply(
            Request {
                scope: edited,
                offset: 0,
            },
            page(0, 1),
        );
        snapshot.apply(
            Request {
                scope: original,
                offset: 39,
            },
            page(39, 50),
        );
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.scope.map(|scope| scope.identity), Some(2));
    }
}
