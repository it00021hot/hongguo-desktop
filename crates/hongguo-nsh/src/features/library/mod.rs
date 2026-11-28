mod actions;
mod data;
mod date;
mod feed;
mod folder_card;
mod folder_creation;
mod folder_membership;
mod folder_move;
mod folder_settings;
mod folders;
use hongguo_service::library_service::model;
mod loading;
mod metadata;
mod notices;
mod posters;
pub(crate) mod presentation;
mod rows;
mod toolbar;

use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui::{self, ButtonVariant},
};
use hongguo_service::library_service::{BookshelfFolder, FolderMembership, RemovalMode};
use model::{Entry, Query};
pub(crate) use notices::NoticePreferences;
use nsh_core::prelude::*;
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Follows,
    Collections,
    Liked,
    Reservations,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum CollectionFolder {
    #[default]
    All,
    Index,
    Official(i64),
}

impl CollectionFolder {
    fn includes(self, entry: &Entry, category: i64) -> bool {
        match self {
            Self::All => category == 0 || entry.content_type == category,
            Self::Index => false,
            Self::Official(_) => true,
        }
    }
}

impl From<Kind> for hongguo_service::library_service::Kind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Follows => Self::Follows,
            Kind::Collections => Self::Collections,
            Kind::Liked => Self::Liked,
            Kind::Reservations => Self::Reservations,
        }
    }
}

impl Kind {
    fn memory_name(self) -> &'static str {
        match self {
            Self::Follows => "follows",
            Self::Collections => "collections",
            Self::Liked => "liked",
            Self::Reservations => "reservations",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Follows => "nav.follows.title",
            Self::Collections => "nav.collections.title",
            Self::Liked => "nav.liked.title",
            Self::Reservations => "nav.reservations.title",
        }
    }
    fn route(self) -> Route {
        match self {
            Self::Follows => Route::Follows,
            Self::Collections => Route::Collections,
            Self::Liked => Route::Liked,
            Self::Reservations => Route::Reservations,
        }
    }
    fn action(self) -> &'static str {
        match self {
            Self::Follows => "follow.unfollow",
            Self::Collections => "collections.remove",
            Self::Liked => "liked.undo",
            Self::Reservations => "reservation.cancel",
        }
    }
    fn search(self) -> &'static str {
        match self {
            Self::Follows => "nav.browse.title",
            Self::Collections => "collections.searchPlaceholder",
            Self::Liked => "liked.searchPlaceholder",
            Self::Reservations => "reservation.searchPlaceholder",
        }
    }
    fn empty(self) -> &'static str {
        match self {
            Self::Follows => "follow.empty",
            Self::Collections => "collections.empty",
            Self::Liked => "liked.empty",
            Self::Reservations => "reservation.empty",
        }
    }
}

pub(crate) fn page(session: SessionHandle, nav: Navigation, kind: Kind) -> View {
    let logged_in = crate::view_memory::cached("library-login", move || {
        computed(move || {
            session.identity();
            session.with(|session| session.state.settings().account.is_some())
        })
    });
    if kind == Kind::Follows {
        return crate::shell::content(nav, move || tr(kind.title()))
            .child(notices::build(session, nav, logged_in));
    }
    if kind == Kind::Reservations {
        return crate::shell::content(nav, move || tr(kind.title()))
            .child(content(session, nav, kind, None));
    }
    crate::shell::content(nav, move || tr(kind.title()))
        .child(when(
            move || logged_in.get(),
            move || content(session, nav, kind, None),
        ))
        .child(when(
            move || !logged_in.get(),
            move || {
                view()
                    .p_4()
                    .gap_3()
                    .child(ui::status(move || tr("settings.loginPrompt")))
                    .child(
                        ui::button(move || tr("settings.loginAction"), ButtonVariant::Primary)
                            .on_press(move || nav.push(Route::Account)),
                    )
            },
        ))
}

fn content(
    session: SessionHandle,
    nav: Navigation,
    kind: Kind,
    leading: Option<View>,
) -> impl IntoElement {
    let memory = crate::view_memory::scope(kind.memory_name());
    let data::Model {
        online,
        query,
        search,
        context,
        result,
    } = data::load(session, nav, kind);
    let category = memory.signal("category", 0_i64);
    let folder_index = memory.signal("folder-index", false);
    let folder_detail = memory.signal("folder-detail", None::<BookshelfFolder>);
    let collection_folder = computed(move || {
        let identity = context.identity.get();
        if let Some(id) = query.with(|query| {
            (query.identity == Some(identity))
                .then_some(query.folder_id)
                .flatten()
        }) {
            CollectionFolder::Official(id)
        } else if folder_index.get() {
            CollectionFolder::Index
        } else {
            CollectionFolder::All
        }
    });
    let busy = signal(false);
    let managing = signal(false);
    let selected = signal(HashSet::<String>::new());
    let feedback = signal(SharedString::default());
    let membership = folder_membership::Membership {
        session,
        context,
        busy,
        feedback,
    };
    let items = feed::current_items(result, query, context);
    let cards = metadata::Cards::new(context.identity);
    let categorized = computed(move || {
        let category = category.get();
        let folder = collection_folder.get();
        items.with(|items| {
            items
                .shared_iter()
                .filter(|item| kind != Kind::Collections || folder.includes(item, category))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let incomplete_search = computed(move || {
        matches!(kind, Kind::Collections | Kind::Liked)
            && !search.with(|query| query.trim().is_empty())
            && !cards.searching(categorized, search)
            && cards.search_incomplete(categorized)
    });
    let visible = computed(move || {
        let filter = search.get().trim().to_lowercase();
        categorized.with(|items| cards.filter(items, &filter))
    });
    if matches!(kind, Kind::Collections | Kind::Liked) {
        cards.drive(
            session,
            context,
            move || kind == Kind::Liked || collection_folder.get() != CollectionFolder::Index,
            visible,
            categorized,
            search,
        );
        effect(move || {
            collection_folder.get();
            category.get();
            search.get();
            if context.active.get() {
                cards.reset();
            }
        });
    }
    effect(move || {
        result.with(|page| {
            if selected.with_untracked(HashSet::is_empty) {
                return;
            }
            selected.update(|selected| {
                selected.retain(|key| page.is_some_and(|page| page.items.contains_key(key)));
            });
        });
    });
    let remove = move |entries: Vec<Entry>| {
        if busy.get_untracked() || result.loading() || entries.is_empty() {
            return;
        }
        busy.set(true);
        feedback.set(SharedString::default());
        let requested = entries.len();
        let identity = session.identity();
        let state = session.with(|session| session.state.clone());
        let mode = if managing.get_untracked() {
            RemovalMode::Selection {
                online: query.with(|query| query.online),
            }
        } else {
            RemovalMode::Row
        };
        spawn(async move {
            let outcome = bridge::request(hongguo_service::library_service::remove(
                state,
                kind.into(),
                entries,
                mode,
            ))
            .await;
            if session.identity() != identity {
                busy.set(false);
                return;
            }
            match outcome {
                Ok(outcome) => {
                    let page_offsets_changed = matches!(kind, Kind::Reservations | Kind::Liked)
                        && !outcome.removed.is_empty();
                    let removed: HashSet<&str> =
                        outcome.removed.iter().map(String::as_str).collect();
                    if !removed.is_empty() {
                        if let Some(mut page) = result.get() {
                            page.items
                                .retain(|item| !removed.contains(item.key.as_str()));
                            result.mutate(page);
                        }
                        selected.update(|keys| keys.retain(|key| !removed.contains(key.as_str())));
                    }
                    if !outcome.errors.is_empty() {
                        let mut messages = Vec::new();
                        for error in &outcome.errors {
                            let message = bridge::Failure::from(error.clone()).text();
                            if !messages.contains(&message) {
                                messages.push(message);
                            }
                        }
                        feedback.set(
                            tr("batch.partialFail")
                                .replace(
                                    "{error}",
                                    &format!(
                                        "{}/{requested}: {}",
                                        outcome.errors.len(),
                                        messages.join("; ")
                                    ),
                                )
                                .into(),
                        );
                    } else {
                        managing.set(false);
                        feedback.set(
                            tr("batch.done")
                                .replace("{count}", &outcome.removed.len().to_string())
                                .into(),
                        );
                    }
                    if page_offsets_changed {
                        let online = query.with(|query| query.online);
                        query.set(Query {
                            identity: Some(identity),
                            online,
                            ..Default::default()
                        });
                        session.notify_library();
                    }
                }
                Err(error) => feedback.set(error.text()),
            }
            busy.set(false);
        });
    };
    let reset = move || {
        let current = query
            .get_untracked()
            .for_identity(context.identity.get_untracked());
        query.set(Query {
            identity: current.identity,
            folder_id: current.folder_id,
            online: current.online,
            ..Default::default()
        });
        selected.update(HashSet::clear);
        result.refetch();
    };
    let play = move |entry| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        feedback.set(SharedString::default());
        let state = session.with(|session| session.clone());
        let identity = session.identity();
        spawn(async move {
            match bridge::request(actions::start_playback(state, entry)).await {
                Ok((id, episode))
                    if session.identity() == identity
                        && nav
                            .routes()
                            .with(|routes| routes.last() == Some(&kind.route())) =>
                {
                    nav.push(Route::Player(id, episode))
                }
                Ok(_) => {}
                Err(error) => feedback.set(error.text()),
            }
            busy.set(false);
        });
    };
    let loading_more = computed(move || {
        result.loading()
            && query.with(|query| {
                !query.previous.is_empty()
                    && result.with(|page| {
                        page.is_some_and(|page| {
                            page.source.as_ref() != Some(&query.source(context.identity.get()))
                        })
                    })
            })
    });
    let rows = rows::Rows {
        kind,
        cards,
        nav,
        items: visible,
        busy,
        managing,
        selected,
        has_more: computed(move || result.with(|page| page.is_some_and(|page| page.has_more))),
        loading_more,
        scroll_scope: computed(move || {
            (
                session.identity(),
                query.get().online,
                search.get(),
                category.get(),
                collection_folder.get(),
            )
        }),
    };
    let choose_folder = move |folder| {
        if busy.get_untracked() || collection_folder.get_untracked() == folder {
            return;
        }
        let current = query
            .get_untracked()
            .for_identity(context.identity.get_untracked());
        let folder_id = match folder {
            CollectionFolder::Official(id) => Some(id),
            _ => None,
        };
        folder_index.set(folder == CollectionFolder::Index);
        search.set(String::new());
        managing.set(false);
        feedback.set(SharedString::default());
        if current.folder_id != folder_id {
            query.set(Query {
                identity: current.identity,
                folder_id,
                online: current.online,
                ..Default::default()
            });
        }
        selected.update(HashSet::clear);
    };
    if kind == Kind::Collections {
        nav.on_back(
            computed(move || {
                context.active.get()
                    && matches!(collection_folder.get(), CollectionFolder::Official(_))
            }),
            move || choose_folder(CollectionFolder::Index),
        );
    }
    let open_folder = move |folder: BookshelfFolder| {
        if !busy.get_untracked() {
            let id = folder.id;
            folder_detail.set(Some(folder));
            choose_folder(CollectionFolder::Official(id));
        }
    };
    let toolbar_state = toolbar::Toolbar {
        kind,
        query,
        search,
        category,
        collection_folder,
        online,
        busy,
        managing,
        selected,
        counts: computed(move || {
            if !context.logged_in.get() {
                return None;
            }
            result.with(|page| {
                page.filter(|page| {
                    query.with(|query| page.belongs_to(query, context.identity.get()))
                })
                .and_then(|page| page.reservation_counts)
            })
        }),
        logged_in: context.logged_in,
    };
    let toolbar = toolbar::build(toolbar_state, choose_folder, leading);
    presentation::frame(kind, toolbar, context.logged_in, nav, move || {
        let body = move || {
            view()
                .flex_1()
                .min_h_0()
                .gap(move || {
                    px(if matches!(kind, Kind::Collections | Kind::Liked) {
                        12.0
                    } else {
                        16.0
                    })
                })
                .child(dynamic(move || {
                    let CollectionFolder::Official(id) = collection_folder.get() else {
                        return view();
                    };
                    let Some(folder) = folder_detail.get().filter(|folder| folder.id == id) else {
                        return view();
                    };
                    let options = folder_settings::action(
                        membership,
                        folder.clone(),
                        move |name, published| {
                            folder_detail.update(|value| {
                                if let Some(folder) =
                                    value.as_mut().filter(|folder| folder.id == id)
                                {
                                    folder.name = name;
                                    folder.published = Some(published);
                                }
                            });
                            reset();
                        },
                        move || choose_folder(CollectionFolder::Index),
                    );
                    let folder = stored(folder);
                    view()
                        .gap_2()
                        .child(when(
                            || !is_desktop(),
                            move || {
                                ui::button(move || tr("nav.back"), ButtonVariant::Secondary)
                                    .self_start()
                                    .disabled(busy)
                                    .on_press(move || nav.back())
                            },
                        ))
                        .child(
                            view()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .px_3()
                                .py_3()
                                .max_w(px(760.0))
                                .rounded(ui::radii::card)
                                .bg(move || nsh_base::theme().card)
                                .style_when(is_desktop, |style| {
                                    style
                                        .max_w(percentage(1.0))
                                        .px_0()
                                        .pt_1()
                                        .pb_3()
                                        .rounded(px(0.0))
                                        .bg(move || nsh_base::theme().background)
                                        .border_b_1()
                                        .border_color(move || nsh_base::theme().border)
                                })
                                .child(
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .gap_3()
                                        .min_w(px(0.0))
                                        .child(
                                            view()
                                                .size(px(40.0))
                                                .items_center()
                                                .justify_center()
                                                .rounded(ui::radii::control)
                                                .bg(move || nsh_base::theme().muted)
                                                .child(
                                                    ui::icon(ui::Icon::Folder)
                                                        .size(px(20.0))
                                                        .text_color(move || {
                                                            nsh_base::theme().primary
                                                        }),
                                                ),
                                        )
                                        .child(
                                            view()
                                                .min_w(px(0.0))
                                                .gap_1()
                                                .child(
                                                    text(move || {
                                                        folder.with(|folder| folder.name.clone())
                                                    })
                                                    .text_lg()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .number_of_lines(1),
                                                )
                                                .child(
                                                    text(move || {
                                                        result.with(|page| {
                                                            page.filter(|page| {
                                                                page.belongs_to(
                                                                    &query.get(),
                                                                    context.identity.get(),
                                                                )
                                                            })
                                                            .and_then(|page| page.folder.as_ref())
                                                            .map_or_else(
                                                                || {
                                                                    folder.with(|folder| {
                                                                        folder_card::summary(folder)
                                                                    })
                                                                },
                                                                folder_card::summary,
                                                            )
                                                        })
                                                    })
                                                    .text_xs()
                                                    .text_color(move || {
                                                        nsh_base::theme().muted_foreground
                                                    }),
                                                ),
                                        ),
                                )
                                .child(
                                    view()
                                        .flex_row()
                                        .flex_wrap()
                                        .items_center()
                                        .gap_2()
                                        .child(when(
                                            move || is_desktop() && !managing.get(),
                                            move || {
                                                presentation::library_search(search, move || {
                                                    tr("collections.searchInFolder")
                                                })
                                                .w(px(190.0))
                                            },
                                        ))
                                        .child(when(
                                            move || !managing.get(),
                                            move || {
                                                folder_membership::add(membership, id, items, reset)
                                            },
                                        ))
                                        .child(
                                            presentation::library_action(
                                                move || {
                                                    tr(if managing.get() {
                                                        "batch.exitManage"
                                                    } else {
                                                        "batch.manage"
                                                    })
                                                },
                                                None,
                                            )
                                            .disabled(busy)
                                            .on_press(
                                                move || {
                                                    managing
                                                        .update(|managing| *managing = !*managing);
                                                    selected.update(HashSet::clear);
                                                },
                                            ),
                                        )
                                        .child(options),
                                ),
                        )
                }))
                .child(when(
                    move || result.loading() && items.with(model::EntryRows::is_empty),
                    move || loading::entries(kind),
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
                            reset,
                        )
                    },
                ))
                .child(when(
                    move || result.with(|page| page.is_some_and(|page| page.warning.is_some())),
                    move || {
                        ui::status(move || {
                            result.with(|page| {
                                page.and_then(|page| page.warning.as_ref())
                                    .map_or_else(SharedString::default, |error| {
                                        bridge::Failure::from(error.clone()).text()
                                    })
                            })
                        })
                    },
                ))
                .child(when(
                    move || {
                        !result.loading()
                            && result.error().is_none()
                            && visible.with(Vec::is_empty)
                            && !incomplete_search.get()
                    },
                    move || {
                        ui::status(move || {
                            if !search.with(|value| value.trim().is_empty()) {
                                if matches!(kind, Kind::Collections | Kind::Liked)
                                    && cards.searching(categorized, search)
                                {
                                    tr(if kind == Kind::Liked {
                                        "liked.searchLoading"
                                    } else {
                                        "collections.searchLoading"
                                    })
                                } else if kind == Kind::Liked && rows.has_more.get() {
                                    tr("feed.loadingMore")
                                } else {
                                    tr("common.searchNoMatch")
                                }
                            } else {
                                tr(kind.empty())
                            }
                        })
                    },
                ))
                .child(rows::build(rows, remove, play, move || {
                    feed::advance(result, query, context)
                }))
                .child(when(incomplete_search, move || {
                    let message = if kind == Kind::Liked {
                        "liked.searchIncomplete"
                    } else {
                        "collections.searchIncomplete"
                    };
                    super::error_state(move || tr(message), move || cards.retry_search())
                }))
                .child(when(
                    move || {
                        (kind == Kind::Liked
                            || matches!(collection_folder.get(), CollectionFolder::Official(_)))
                            && rows.has_more.get()
                            && result.error().is_none()
                    },
                    move || {
                        ui::button(
                            move || {
                                tr(if loading_more.get() {
                                    "feed.loadingMore"
                                } else {
                                    "follow.loadMore"
                                })
                            },
                            ButtonVariant::Secondary,
                        )
                        .disabled(move || result.loading() || busy.get())
                        .on_press(move || feed::advance(result, query, context))
                    },
                ))
                .child(toolbar::batch(rows, remove, move || {
                    let CollectionFolder::Official(id) = collection_folder.get_untracked() else {
                        return view();
                    };
                    let has_selection = computed(move || {
                        selected.with(|keys| {
                            !keys.is_empty()
                                && visible.with(|entries| {
                                    entries.iter().any(|entry| keys.contains(&entry.key))
                                })
                        })
                    });
                    let picked = move || {
                        selected.with(|keys| {
                            if keys.is_empty() {
                                return Vec::new();
                            }
                            visible.with(|entries| {
                                entries
                                    .iter()
                                    .filter(|entry| keys.contains(&entry.key))
                                    .map(|entry| Arc::as_ref(entry).clone())
                                    .collect::<Vec<_>>()
                            })
                        })
                    };
                    let done = move || {
                        managing.set(false);
                        reset();
                    };
                    view()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(folder_move::action(
                            membership,
                            id,
                            picked,
                            has_selection,
                            done,
                        ))
                        .child(
                            ui::controls::compact_button(
                                move || tr("collections.folderRemove"),
                                ButtonVariant::Secondary,
                            )
                            .disabled(move || busy.get() || !has_selection.get())
                            .on_press(move || {
                                membership.change(id, FolderMembership::Remove, picked(), done)
                            }),
                        )
                }))
                .child(when(
                    move || {
                        kind == Kind::Follows
                            && result.with(|page| page.is_some_and(|page| page.has_more))
                    },
                    move || {
                        ui::button(
                            move || {
                                tr(if loading_more.get() {
                                    "feed.loadingMore"
                                } else {
                                    "follow.loadMore"
                                })
                            },
                            ButtonVariant::Secondary,
                        )
                        .disabled(move || result.loading() || busy.get())
                        .on_press(move || feed::advance(result, query, context))
                    },
                ))
        };
        view()
            .relative()
            .flex_1()
            .min_h_0()
            .child(when(
                move || collection_folder.get() == CollectionFolder::Index,
                move || folders::index(session, context, busy, open_folder),
            ))
            .child(when(
                move || collection_folder.get() != CollectionFolder::Index,
                body,
            ))
            .child(ui::transient_notice(feedback))
    })
}
