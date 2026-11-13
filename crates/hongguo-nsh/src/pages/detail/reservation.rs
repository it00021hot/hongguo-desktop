use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    pages::SessionHandle,
};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct Reservation {
    pub selected: Memo<bool>,
    pub busy: Signal<bool>,
    pub message: Memo<SharedString>,
    pub failed: Memo<bool>,
    submit: Stored<Box<dyn Fn()>>,
    retry: Stored<Box<dyn Fn()>>,
}

impl Reservation {
    pub fn submit(self) {
        self.submit.with(|submit| submit());
    }
    pub fn retry(self) {
        self.retry.with(|retry| retry());
    }
}

pub(super) fn observe(
    session: SessionHandle,
    nav: Navigation,
    id: Stored<String>,
    active: Memo<bool>,
) -> Reservation {
    let loaded = resource(
        move || {
            let identity = session.identity();
            let revision = session.library();
            (active.get() && session.with(|session| session.state.settings().account.is_some()))
                .then(|| (id.get(), identity, revision))
        },
        move |(id, _, _), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let env = state.api_env();
                let offline = hongguo_api::reservation::fetch_reservations(false, &env).await?;
                let online = hongguo_api::reservation::fetch_reservations(true, &env).await?;
                Ok(offline
                    .items
                    .iter()
                    .chain(&online.items)
                    .any(|item| item.series_id == id))
            })
        },
    );
    controller(
        loaded,
        computed(move || session.identity()),
        move || {
            if session.with(|session| session.state.settings().account.is_some()) {
                true
            } else {
                nav.push(Route::Account);
                false
            }
        },
        move |selected| {
            let state = session.with(|session| session.state.clone());
            let id = id.get();
            bridge::request(async move {
                hongguo_api::reservation::reserve_series(&id, selected, &state.api_env()).await
            })
        },
        move || session.notify_library(),
    )
}

pub(super) fn controller<F: std::future::Future<Output = Result<(), bridge::Failure>> + 'static>(
    loaded: Resource<bool, bridge::Failure>,
    identity: Memo<u64>,
    authorize: impl Fn() -> bool + 'static,
    write: impl Fn(bool) -> F + 'static,
    changed: impl Fn() + 'static,
) -> Reservation {
    let override_value = signal(None);
    let busy = signal(false);
    let notice = signal(SharedString::default());
    let failed = signal(false);
    let generation = stored(identity.get_untracked());
    effect(move || {
        let next = identity.get();
        if generation.replace(next) != next {
            override_value.set(None);
            busy.set(false);
            notice.set(SharedString::default());
            failed.set(false);
        }
    });
    effect(move || {
        if !loaded.loading() && loaded.error().is_none() && loaded.get().is_some() {
            override_value.set(None);
        }
    });
    let selected = computed(move || {
        override_value.get().unwrap_or_else(|| {
            if loaded.loading() || loaded.error().is_some() {
                false
            } else {
                loaded.get().unwrap_or(false)
            }
        })
    });
    let authorize = stored(authorize);
    let write = stored(write);
    let changed = stored(changed);
    let submit = stored(Box::new(move || {
        if !authorize.with(|authorize| authorize()) || busy.get_untracked() {
            return;
        }
        let next = !selected.get_untracked();
        let expected_identity = identity.get_untracked();
        busy.set(true);
        notice.set(SharedString::default());
        failed.set(false);
        let pending = write.with(|write| write(next));
        spawn(async move {
            let result = pending.await;
            if identity.get_untracked() != expected_identity {
                return;
            }
            busy.set(false);
            match result {
                Ok(()) => {
                    override_value.set(Some(next));
                    notice.set(tr(if next {
                        "player.interact.reserved"
                    } else {
                        "player.interact.unreserved"
                    }));
                    changed.with(|changed| changed());
                }
                Err(error) => {
                    failed.set(true);
                    notice.set(error.text());
                }
            }
        });
    }) as Box<dyn Fn()>);
    Reservation {
        selected,
        busy,
        failed: computed(move || {
            failed.get() || (notice.get().is_empty() && loaded.error().is_some())
        }),
        message: computed(move || {
            if !notice.get().is_empty() {
                notice.get()
            } else {
                loaded
                    .error()
                    .map_or_else(SharedString::default, |error| error.text())
            }
        }),
        submit,
        retry: stored(Box::new(move || {
            notice.set(SharedString::default());
            failed.set(false);
            loaded.refetch();
        }) as Box<dyn Fn()>),
    }
}

pub(super) fn preview() -> Reservation {
    let loaded = resource(|| Some(()), |_, _| std::future::ready(Ok(false)));
    controller(
        loaded,
        computed(|| 0),
        || true,
        |_| std::future::ready(Ok(())),
        || {},
    )
}
