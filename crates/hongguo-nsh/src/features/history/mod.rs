mod data;
mod list;
mod model;
pub(crate) mod presentation;
use self::{data as history_data, list as history};
use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;

type HistoryFeed = Resource<(u64, Vec<model::HistoryItem>), bridge::Failure>;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> View {
    content(session, nav, data(session, nav), local_data(session, nav))
}

fn data(session: SessionHandle, nav: Navigation) -> HistoryFeed {
    crate::view_memory::cached("history", move || {
        let state = session.with(|session| session.state.clone());
        resource(
            move || {
                nav.routes()
                    .with(|routes| routes.last() == Some(&Route::History))
                    .then(|| (session.identity(), session.library()))
            },
            move |(identity, _), _| {
                let state = state.clone();
                bridge::request(async move {
                    history_data::load(state)
                        .await
                        .map(|items| (identity, items))
                })
            },
        )
    })
}

fn local_data(session: SessionHandle, nav: Navigation) -> HistoryFeed {
    crate::view_memory::cached("history-local", move || {
        resource(
            move || {
                if !nav
                    .routes()
                    .with(|routes| routes.last() == Some(&Route::History))
                {
                    return None;
                }
                let identity = session.identity();
                let revision = session.library();
                session
                    .with(|session| history_data::identity(&session.state))
                    .map(|account| (identity, revision, account))
            },
            move |(identity, _, account), _| {
                let state = session.with(|session| session.state.clone());
                bridge::request(async move {
                    hongguo_service::history_service::list_local(&state, account)
                        .await
                        .map(|items| {
                            (
                                identity,
                                items.into_iter().map(model::HistoryItem::from).collect(),
                            )
                        })
                })
            },
        )
    })
}

fn content(
    session: SessionHandle,
    nav: Navigation,
    result: HistoryFeed,
    local: HistoryFeed,
) -> View {
    let owner = Owner::current();
    let busy = signal(false);
    let deleted = signal(0_u64);
    let feedback = signal(SharedString::default());
    let user = computed(move || {
        session.identity();
        session.with(|session| history_data::identity(&session.state))
    });
    let resolving = signal(false);
    effect(move || {
        session.identity();
        feedback.set(SharedString::default());
    });
    let open = move |item: &model::HistoryItem| {
        if busy.get_untracked() || resolving.get_untracked() {
            return;
        }
        resolving.set(true);
        let state = session.with(|session| session.clone());
        let id = item.id.clone();
        let episode = item.episode;
        let at = hongguo_common::model::playback::resume_position(
            item.position_ms.max(0) as f64 / 1000.0,
            item.duration_ms.max(0) as f64 / 1000.0,
        );
        let identity = session.identity();
        spawn(async move {
            let outcome = bridge::request(crate::features::player::resolve_history_start(
                state.clone(),
                id,
                episode,
            ))
            .await;
            if resolving.try_get().is_none() {
                return;
            }
            resolving.set(false);
            if session.identity() != identity
                || !nav
                    .routes()
                    .with(|routes| routes.last() == Some(&Route::History))
            {
                return;
            }
            match outcome {
                Ok(id) => {
                    match state
                        .personal
                        .remember_position(&state.state, id.clone(), episode, at)
                    {
                        Ok(()) => nav.push(Route::Player(id, episode)),
                        Err(error) => feedback.set(bridge::Failure::from(error).text()),
                    }
                }
                Err(error) => feedback.set(error.text()),
            }
        });
    };
    let remove = move |entries: Vec<model::HistoryItem>| {
        if busy.get_untracked() || entries.is_empty() {
            return;
        }
        busy.set(true);
        feedback.set(SharedString::default());
        let identity = session.identity();
        let expected_user = user.get_untracked();
        let selected_count = entries.len();
        let state = session.with(|session| session.state.clone());
        owner.run(move || {
            spawn(async move {
                let outcome =
                    bridge::submit(history_data::remove(state, entries, expected_user)).await;
                if session.identity() == identity {
                    match outcome {
                        Ok(()) => {
                            deleted.update(|revision| *revision += 1);
                            session.notify_library();
                            result.refetch();
                            local.refetch();
                            feedback.set(
                                tr("batch.done")
                                    .replace("{count}", &selected_count.to_string())
                                    .into(),
                            );
                        }
                        Err(error) => {
                            feedback.set(error.text());
                            result.refetch();
                            local.refetch();
                        }
                    }
                }
                busy.set(false);
            });
        });
    };
    let has_current = computed(move || {
        result.with(|items| items.is_some_and(|(identity, _)| *identity == session.identity()))
    });
    let has_local = computed(move || {
        local.with(|items| {
            items.is_some_and(|(identity, items)| {
                *identity == session.identity() && !items.is_empty()
            })
        })
    });
    let visible = computed(move || {
        let identity = session.identity();
        result
            .with(|items| {
                items
                    .filter(|(owner, _)| *owner == identity)
                    .map(|(_, items)| items.clone())
            })
            .or_else(|| {
                local.with(|items| {
                    items
                        .filter(|(owner, _)| *owner == identity)
                        .map(|(_, items)| items.clone())
                })
            })
            .unwrap_or_default()
    });
    crate::shell::content(nav, move || tr("nav.history.title")).child(
        view()
            .relative()
            .flex_1()
            .child(when(
                move || resolving.get(),
                move || ui::status(move || tr("common.resolving")),
            ))
            .child(when(
                move || result.loading() && !has_current.get() && !has_local.get(),
                crate::features::history::presentation::history_loading,
            ))
            .child(when(
                move || result.error().is_some(),
                move || {
                    super::error_state(
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
                move || (has_current.get() && result.error().is_none()) || has_local.get(),
                move || {
                    history::content(
                        visible,
                        hongguo_common::utils::time::now_ms(),
                        history::HistoryEvents {
                            open: Box::new(open),
                            remove: Some(Box::new(remove)),
                            busy: computed(move || busy.get() || resolving.get()),
                            can_manage: computed(move || {
                                has_current.get() && result.error().is_none()
                            }),
                            reset: computed(move || (session.identity(), deleted.get())),
                        },
                    )
                },
            ))
            .child(ui::transient_notice(feedback)),
    )
}
