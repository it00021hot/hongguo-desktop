use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    pages::SessionHandle,
};
use hongguo_api::calendar::CalendarItem;
use nsh_core::prelude::*;
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(super) struct Reservations {
    pub(super) reserved: Signal<HashSet<String>>,
    pub(super) pending: Signal<HashSet<String>>,
    pub(super) message: Signal<SharedString>,
}

impl Reservations {
    pub(super) fn observe(session: SessionHandle) -> Self {
        let state = Self {
            reserved: signal(HashSet::new()),
            pending: signal(HashSet::new()),
            message: signal(SharedString::default()),
        };
        let identity = stored(session.identity());
        effect(move || {
            if identity.replace(session.identity()) != session.identity() {
                state.reserved.set(HashSet::new());
                state.pending.set(HashSet::new());
                state.message.set(SharedString::default());
            }
        });
        state
    }

    pub(super) fn reserve(self, session: SessionHandle, nav: Navigation, item: CalendarItem) {
        let state = session.with(|session| session.state.clone());
        if state.settings().account.is_none() {
            nav.push(Route::Account);
            return;
        }
        if item.has_subscribed
            || self.reserved.with(|items| items.contains(&item.series_id))
            || self.pending.with(|items| items.contains(&item.series_id))
        {
            return;
        }
        let identity = session.identity();
        self.pending.update(|items| {
            items.insert(item.series_id.clone());
        });
        self.message.set(SharedString::default());
        spawn(async move {
            let id = item.series_id;
            let request_id = id.clone();
            let result = bridge::request(async move {
                hongguo_api::reservation::reserve_series(&request_id, true, &state.api_env()).await
            })
            .await;
            if identity != session.identity() {
                return;
            }
            self.pending.update(|items| {
                items.remove(&id);
            });
            match result {
                Ok(()) => {
                    self.reserved.update(|items| {
                        items.insert(id);
                    });
                    self.message.set(tr("reservation.done"));
                    session.notify_library();
                }
                Err(error) => self.message.set(error.text()),
            }
        });
    }
}
