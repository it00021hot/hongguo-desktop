mod actions;
#[cfg(test)]
pub(crate) mod cache_tests;
mod data;
mod date;
mod feed;
mod fetch;
mod fixtures;
mod model;
mod notices;
mod posters;
mod presentation;
mod rows;
mod toolbar;

#[cfg(test)]
mod notices_tests;

use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui::{self, ButtonVariant},
};
use model::{Entry, Query};
pub(crate) use notices::NoticePreferences;
use nsh_core::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Follows,
    Collections,
    Liked,
    Reservations,
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

pub(crate) fn preview(nav: Navigation, kind: Kind) -> Screen {
    fixtures::page(nav, kind)
}

pub(crate) fn page(session: SessionHandle, nav: Navigation, kind: Kind) -> Screen {
    let logged_in = crate::view_memory::cached("library-login", move || {
        computed(move || {
            session.identity();
            session.with(|session| session.state.settings().account.is_some())
        })
    });
    if kind == Kind::Follows {
        return ui::page(nav, move || tr(kind.title()))
            .child(notices::build(session, nav, logged_in));
    }
    if kind == Kind::Reservations {
        return ui::page(nav, move || tr(kind.title())).child(content(session, nav, kind));
    }
    ui::page(nav, move || tr(kind.title()))
        .child(when(
            move || logged_in.get(),
            move || content(session, nav, kind),
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

fn content(session: SessionHandle, nav: Navigation, kind: Kind) -> impl IntoElement {
    let memory = crate::view_memory::scope(kind.memory_name());
    let data::Model {
        online,
        query,
        search,
        context,
        result,
    } = data::load(session, nav, kind);
    let category = memory.signal("category", 0_i64);
    let busy = signal(false);
    let managing = signal(false);
    let selected = signal(Vec::<String>::new());
    let feedback = signal(SharedString::default());
    let items = feed::current_items(result, query, context);
    let visible = computed(move || {
        let filter = search.get().trim().to_lowercase();
        items.with(|items| {
            items
                .iter()
                .filter(|item| {
                    (filter.is_empty()
                        || item.title.to_lowercase().contains(&filter)
                        || item.series_id.to_lowercase().contains(&filter))
                        && (category.get() == 0 || item.content_type == category.get())
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    effect(move || {
        let keys = result.with(|page| {
            page.map(|page| {
                page.items
                    .iter()
                    .map(|item| item.key.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        });
        selected.update(|selected| selected.retain(|key| keys.contains(key)));
    });
    let remove = move |entries: Vec<Entry>| {
        if busy.get_untracked() || entries.is_empty() {
            return;
        }
        busy.set(true);
        feedback.set(SharedString::default());
        let requested = entries.len();
        let identity = session.identity();
        let state = session.with(|session| session.state.clone());
        let online = query.get_untracked().online;
        spawn(async move {
            let outcome = bridge::request(actions::remove(state, kind, entries, online)).await;
            if session.identity() != identity {
                busy.set(false);
                return;
            }
            match outcome {
                Ok(outcome) => {
                    if let Some(mut page) = result.get() {
                        page.items
                            .retain(|item| !outcome.removed.contains(&item.key));
                        result.mutate(page);
                    }
                    selected.update(|keys| keys.retain(|key| !outcome.removed.contains(key)));
                    if !outcome.errors.is_empty() {
                        let mut messages = Vec::new();
                        for error in &outcome.errors {
                            let message = error.text();
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
                }
                Err(error) => feedback.set(error.text()),
            }
            busy.set(false);
        });
    };
    let reset = move || {
        let online = query.get_untracked().online;
        query.set(Query {
            online,
            ..Default::default()
        });
        selected.set(Vec::new());
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
    let rows = rows::Rows {
        kind,
        nav,
        items: visible,
        busy,
        managing,
        selected,
        has_more: computed(move || result.with(|page| page.is_some_and(|page| page.has_more))),
        scroll_scope: computed(move || {
            (
                session.identity(),
                query.get().online,
                search.get(),
                category.get(),
            )
        }),
    };
    let toolbar = toolbar::build(toolbar::Toolbar {
        kind,
        query,
        search,
        category,
        online,
        busy,
        managing,
        selected,
        counts: computed(move || {
            if !context.logged_in.get() {
                return None;
            }
            result.with(|page| {
                page.filter(|page| page.belongs_to(&query.get(), context.identity.get()))
                    .and_then(|page| page.reservation_counts)
            })
        }),
        logged_in: context.logged_in,
    });
    presentation::frame(kind, toolbar, context.logged_in, nav, move || {
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
            .child(ui::status(feedback))
            .child(when(
                move || result.loading() && items.with(Vec::is_empty),
                move || ui::status(move || tr("common.loading")),
            ))
            .child(when(
                move || result.error().is_some(),
                move || {
                    crate::pages::error_state(
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
                                .map_or_else(SharedString::default, |error| error.text())
                        })
                    })
                },
            ))
            .child(when(
                move || {
                    !result.loading() && result.error().is_none() && visible.with(Vec::is_empty)
                },
                move || {
                    ui::status(move || {
                        if search.with(String::is_empty) {
                            tr(kind.empty())
                        } else {
                            tr("common.searchNoMatch")
                        }
                    })
                },
            ))
            .child(rows::build(rows, remove, play, move || {
                feed::advance(result, query, context)
            }))
            .child(toolbar::batch(rows, remove, move |error| {
                feedback.set(error)
            }))
            .child(when(
                move || {
                    kind == Kind::Follows
                        && result.with(|page| page.is_some_and(|page| page.has_more))
                },
                move || {
                    ui::button(move || tr("follow.loadMore"), ButtonVariant::Secondary)
                        .disabled(move || result.loading() || busy.get())
                        .on_press(move || feed::advance(result, query, context))
                },
            ))
            .child(when(
                move || {
                    kind == Kind::Reservations && result.loading() && !items.with(Vec::is_empty)
                },
                move || ui::status(move || tr("feed.loadingMore")),
            ))
    })
}
