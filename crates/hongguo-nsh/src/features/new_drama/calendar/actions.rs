use crate::{
    bridge,
    features::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
};
use hongguo_service::contracts::calendar::CalendarItem;
use nsh_core::prelude::*;
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(in crate::features::new_drama) struct Reservations {
    pub(in crate::features::new_drama) reserved: Signal<HashSet<String>>,
    pub(in crate::features::new_drama) pending: Signal<HashSet<String>>,
    pub(in crate::features::new_drama) message: Signal<SharedString>,
}

impl Reservations {
    pub(in crate::features::new_drama) fn observe(session: SessionHandle) -> Self {
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

    pub(in crate::features::new_drama) fn reserve(
        self,
        session: SessionHandle,
        nav: Navigation,
        item: CalendarItem,
    ) {
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
                hongguo_service::reservation_service::toggle(&request_id, true, &state).await
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
