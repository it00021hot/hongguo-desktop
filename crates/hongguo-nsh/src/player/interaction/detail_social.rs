use super::{
    api,
    model::{Action, Snapshot, Target},
};
use crate::{
    bridge,
    navigation::{Navigation, Route},
    pages::SessionHandle,
};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct DetailSocial {
    pub collected: Memo<bool>,
    pub liked: Memo<bool>,
    pub followed: Memo<bool>,
    pub busy: Memo<bool>,
    pub message: Memo<SharedString>,
    submit: Stored<Box<dyn Fn(Action)>>,
    retry: Stored<Box<dyn Fn()>>,
}

pub(crate) fn observe(
    session: SessionHandle,
    nav: Navigation,
    series: Resource<hongguo_common::model::Series, bridge::Failure>,
    resume: Resource<Option<u32>, bridge::Failure>,
) -> DetailSocial {
    let memory = session.social_memory();
    let account = computed(move || {
        session.identity();
        session.with(|session| {
            session
                .state
                .settings()
                .account
                .map(|account| account.user_id)
                .unwrap_or_default()
        })
    });
    let target = computed(move || {
        series.with(|series| {
            series.and_then(|series| {
                let index = super::super::start_episode(series, resume.get().flatten()).ok()?;
                series
                    .episodes
                    .iter()
                    .find(|episode| episode.vid_index == index)
                    .map(|episode| Target::from_episode(series, episode))
            })
        })
    });
    let loaded = resource(
        move || target.get().map(|target| (target, session.identity())),
        move |(target, _), _| {
            let state = session.with(|session| session.state.clone());
            let logged_in = state.settings().account.is_some();
            bridge::request(api::load(state, target, logged_in))
        },
    );
    let snapshot = signal(Snapshot::default());
    let notice = signal(SharedString::default());
    let identity = stored(session.identity());
    effect(move || {
        let current = session.identity();
        if identity.replace(current) != current {
            snapshot.set(Snapshot::default());
            notice.set(SharedString::default());
        }
    });
    effect(move || {
        if !loaded.loading()
            && loaded.error().is_none()
            && let Some(value) = loaded.get()
            && let Some(target) = target.get()
        {
            snapshot.set(memory.refresh(&account.get(), &target, value, snapshot.get_untracked()));
        }
    });
    let busy = computed(move || {
        loaded.loading()
            || loaded.error().is_some()
            || target.get().is_none()
            || snapshot.get().collect.pending
            || snapshot.get().like.pending
            || snapshot.get().follow.pending
    });
    let submit = stored(Box::new(move |action| {
        let state = session.with(|session| session.state.clone());
        if state.settings().account.is_none() {
            nav.push(Route::Account);
            return;
        }
        if busy.get_untracked() {
            return;
        }
        let Some(target) = target.get_untracked() else {
            return;
        };
        let mut next = snapshot.get_untracked();
        let Some(before) = next.begin(action) else {
            return;
        };
        let selected = next.value(action).selected;
        snapshot.set(next);
        notice.set(SharedString::default());
        let identity = session.identity();
        let account = account.get_untracked();
        spawn(async move {
            let result =
                bridge::request(api::submit(state, target.clone(), action, selected)).await;
            if identity != session.identity() {
                return;
            }
            snapshot.update(|value| value.finish(action, before, result.is_ok()));
            match result {
                Ok(()) => {
                    memory.remember(
                        &account,
                        &target,
                        action,
                        snapshot.get_untracked().value(action),
                    );
                    session.notify_library();
                }
                Err(error) => notice.set(error.text()),
            }
        });
    }) as Box<dyn Fn(Action)>);
    state(
        snapshot,
        busy,
        computed(move || {
            loaded
                .error()
                .map_or_else(|| notice.get(), |error| error.text())
        }),
        submit,
        stored(Box::new(move || {
            notice.set(SharedString::default());
            loaded.refetch();
        }) as Box<dyn Fn()>),
    )
}

pub(crate) fn preview() -> DetailSocial {
    let snapshot = signal(Snapshot::default());
    let submit = stored(Box::new(move |action| {
        snapshot.update(|value| {
            if let Some(before) = value.begin(action) {
                value.finish(action, before, true);
            }
        });
    }) as Box<dyn Fn(Action)>);
    state(
        snapshot,
        computed(|| false),
        computed(SharedString::default),
        submit,
        stored(Box::new(|| {}) as Box<dyn Fn()>),
    )
}

fn state(
    snapshot: Signal<Snapshot>,
    busy: Memo<bool>,
    message: Memo<SharedString>,
    submit: Stored<Box<dyn Fn(Action)>>,
    retry: Stored<Box<dyn Fn()>>,
) -> DetailSocial {
    DetailSocial {
        collected: computed(move || snapshot.get().collect.selected),
        liked: computed(move || snapshot.get().like.selected),
        followed: computed(move || snapshot.get().follow.selected),
        busy,
        message,
        submit,
        retry,
    }
}

impl DetailSocial {
    pub fn retry(self) {
        self.retry.with(|retry| retry());
    }
    pub fn collect(self) {
        self.submit.with(|submit| submit(Action::Collect));
    }
    pub fn like(self) {
        self.submit.with(|submit| submit(Action::Like));
    }
    pub fn follow(self) {
        self.submit.with(|submit| submit(Action::Follow));
    }
}
