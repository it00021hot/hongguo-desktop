mod api;
mod comments;
pub(crate) use comments::{comments_preview, reviews_preview_with_header, reviews_with_header};
pub(super) mod danmaku;
pub(crate) mod detail_social;
mod memory;
mod model;
pub(crate) use memory::SocialMemory;

pub(super) use model::Target;

use crate::{
    bridge,
    navigation::{Navigation, Route},
    pages::SessionHandle,
    ui,
};
use model::{Action, Snapshot};
use nsh_core::prelude::*;

#[derive(Clone, PartialEq)]
struct PanelTarget {
    target: Target,
    account: String,
}

pub(super) fn panel(
    session: SessionHandle,
    nav: Navigation,
    target: impl IntoReactive<Option<Target>>,
    comments_open: Signal<bool>,
) -> impl IntoElement {
    let target = target.into_reactive();
    let memory = session.social_memory();
    view().px_3().gap_2().child(each(
        move || {
            session.identity();
            let account = session.with(|session| {
                session
                    .state
                    .settings()
                    .account
                    .map(|account| account.user_id)
                    .unwrap_or_default()
            });
            target
                .get()
                .map(|target| PanelTarget { target, account })
                .into_iter()
                .collect::<Vec<_>>()
        },
        |panel: &PanelTarget| {
            (
                panel.target.series_id.clone(),
                panel.target.vid.clone(),
                panel.account.clone(),
            )
        },
        move |panel, _| {
            let panel = panel.get_untracked();
            controls(session, nav, panel, memory, comments_open)
        },
    ))
}

fn controls(
    session: SessionHandle,
    nav: Navigation,
    panel: PanelTarget,
    memory: SocialMemory,
    comments_open: Signal<bool>,
) -> impl IntoElement {
    let logged_in = !panel.account.is_empty();
    let target = stored(panel.target);
    let account = stored(panel.account);
    let snapshot = signal(Snapshot::default());
    let message = signal(SharedString::default());
    let loaded = resource(
        || Some(()),
        move |_, _| {
            let state = session.with(|session| session.state.clone());
            let target = target.get();
            bridge::request(api::load(state, target, logged_in))
        },
    );
    effect(move || {
        if !loaded.loading()
            && loaded.error().is_none()
            && let Some(initial) = loaded.get()
        {
            snapshot.set(memory.refresh(
                &account.get(),
                &target.get(),
                initial,
                snapshot.get_untracked(),
            ));
        }
    });
    let submit = move |action| {
        if !logged_in {
            nav.push(Route::Account);
            return;
        }
        if loaded.loading() || loaded.get().is_none() {
            return;
        }
        let mut optimistic = snapshot.get_untracked();
        let Some(before) = optimistic.begin(action) else {
            return;
        };
        snapshot.set(optimistic);
        message.set(SharedString::default());
        let state = session.with(|session| session.state.clone());
        let request = target.get();
        let selected = snapshot.get_untracked().value(action).selected;
        spawn(async move {
            let result = bridge::request(api::submit(state, request, action, selected)).await;
            snapshot.update(|snapshot| snapshot.finish(action, before, result.is_ok()));
            match result {
                Ok(()) => {
                    let value = snapshot.get_untracked().value(action);
                    memory.remember(&account.get(), &target.get(), action, value);
                    session.notify_library();
                }
                Err(error) => message.set(error.text()),
            }
        });
    };
    view()
        .child(crate::player::stage::rail(
            move || {
                let value = snapshot.get();
                let unavailable = loaded.loading() || loaded.get().is_none();
                crate::player::stage::RailState {
                    liked: value.like.selected,
                    collected: value.collect.selected,
                    following: value.follow.selected,
                    show_follow: value.show_follow,
                    likes: value.like.count,
                    collections: value.collect.count,
                    comments: target.with(|target| target.comments),
                    like_pending: unavailable || value.like.pending,
                    collect_pending: unavailable || value.collect.pending,
                    follow_pending: unavailable || value.follow.pending,
                }
            },
            move |action| {
                use crate::player::stage::RailAction;
                match action {
                    RailAction::Comments => comments_open.set(true),
                    RailAction::Share => target.with(|target| {
                        set_clipboard(format!(
                            "{} https://hongguoduanju.com/detail?series_id={}",
                            target.title, target.series_id
                        ))
                    }),
                    RailAction::Like => submit(Action::Like),
                    RailAction::Collect => submit(Action::Collect),
                    RailAction::Follow => submit(Action::Follow),
                }
            },
        ))
        .child(comments::panel(
            session,
            nav,
            target,
            logged_in,
            comments_open,
        ))
        .child(ui::status(message))
        .child(when(
            move || loaded.error().is_some(),
            move || {
                crate::pages::error_state(
                    move || {
                        loaded
                            .error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || loaded.refetch(),
                )
            },
        ))
}
#[cfg(test)]
mod tests;
