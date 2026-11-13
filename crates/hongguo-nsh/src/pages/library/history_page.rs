use super::{history, history_data};
use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;

#[cfg(test)]
#[path = "history_cache_tests.rs"]
mod cache_tests;

pub(super) fn build(session: SessionHandle, nav: Navigation) -> Screen {
    let result = data(session, nav);
    content(session, nav, result)
}

fn data(
    session: SessionHandle,
    nav: Navigation,
) -> Resource<(u64, Vec<super::model::HistoryItem>), bridge::Failure> {
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

fn content(
    session: SessionHandle,
    nav: Navigation,
    result: Resource<(u64, Vec<super::model::HistoryItem>), bridge::Failure>,
) -> Screen {
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
    let open = move |item: &super::model::HistoryItem| {
        if busy.get_untracked() || resolving.get_untracked() {
            return;
        }
        resolving.set(true);
        let state = session.with(|session| session.clone());
        let id = item.id.clone();
        let episode = item.episode;
        let at = item.position_ms.max(0) as f64 / 1000.0;
        let identity = session.identity();
        spawn(async move {
            let outcome = bridge::request(crate::player::resolve_start(state.clone(), id)).await;
            match outcome {
                Ok((id, _))
                    if session.identity() == identity
                        && nav
                            .routes()
                            .with(|routes| routes.last() == Some(&Route::History)) =>
                {
                    match state
                        .personal
                        .remember_position(&state.state, id.clone(), episode, at)
                    {
                        Ok(()) => nav.push(Route::Player(id, episode)),
                        Err(error) => feedback.set(bridge::Failure::from(error).text()),
                    }
                }
                Ok(_) => {}
                Err(error) => feedback.set(error.text()),
            }
            resolving.set(false);
        });
    };
    let remove = move |entries: Vec<super::model::HistoryItem>| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        feedback.set(SharedString::default());
        let identity = session.identity();
        let expected_user = user.get_untracked();
        let state = session.with(|session| session.state.clone());
        spawn(async move {
            let outcome =
                bridge::request(history_data::remove(state, entries, expected_user)).await;
            if session.identity() == identity {
                match outcome {
                    Ok(()) => {
                        deleted.update(|revision| *revision += 1);
                        session.notify_library();
                        result.refetch();
                    }
                    Err(error) => feedback.set(error.text()),
                }
            }
            busy.set(false);
        });
    };
    let has_current = computed(move || {
        result.with(|items| items.is_some_and(|(identity, _)| *identity == session.identity()))
    });
    ui::page(nav, move || tr("nav.history.title")).child(
        view()
            .flex_1()
            .child(ui::status(feedback))
            .child(when(
                move || resolving.get(),
                move || ui::status(move || tr("common.resolving")),
            ))
            .child(when(
                move || result.loading() && !has_current.get(),
                ui::history_loading,
            ))
            .child(when(
                move || result.error().is_some(),
                move || {
                    super::super::error_state(
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
                move || has_current.get() && result.error().is_none(),
                move || {
                    history::content(
                        move || {
                            result.with(|items| {
                                items.map(|(_, items)| items.clone()).unwrap_or_default()
                            })
                        },
                        move || user.get().is_none(),
                        hongguo_common::utils::time::now_ms(),
                        history::HistoryEvents {
                            open: Box::new(open),
                            remove: Some(Box::new(remove)),
                            busy: computed(move || busy.get() || resolving.get()),
                            reset: computed(move || (session.identity(), deleted.get())),
                        },
                    )
                },
            )),
    )
}
