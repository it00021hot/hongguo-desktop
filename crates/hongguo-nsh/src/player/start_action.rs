use crate::{
    bridge,
    navigation::{Navigation, Route},
    pages::SessionHandle,
};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct StartAction {
    session: SessionHandle,
    nav: Navigation,
    origin: Stored<Option<Route>>,
    pending: Signal<Option<String>>,
    notice: Signal<SharedString>,
    task: Stored<Option<nsh_core::element::Spawned<()>>>,
}

pub(crate) fn start_action(session: SessionHandle, nav: Navigation) -> StartAction {
    let action = StartAction {
        session,
        nav,
        origin: stored(nav.routes().get_untracked().last().cloned()),
        pending: signal(None),
        notice: signal(SharedString::default()),
        task: stored(None),
    };
    let identity = stored(session.identity());
    effect(move || {
        let current = session.identity();
        let account_changed = identity.replace(current) != current;
        let active = nav.routes().with(|routes| routes.last().cloned()) == action.origin.get();
        if (account_changed || !active) && action.pending.get_untracked().is_some() {
            action.cancel();
        }
    });
    action
}

impl StartAction {
    pub fn pending(self, id: &str) -> bool {
        self.pending.with(|pending| pending.as_deref() == Some(id))
    }
    pub fn notice(self) -> SharedString {
        self.notice.get()
    }
    pub fn start(self, id: String) {
        if self.pending.get_untracked().as_ref() == Some(&id) {
            return;
        }
        self.cancel();
        self.notice.set(SharedString::default());
        self.pending.set(Some(id.clone()));
        let state = self.session.with(|session| session.clone());
        let identity = self.session.identity();
        let task = spawn(async move {
            let outcome = bridge::request(super::resolve_start(state, id)).await;
            self.pending.set(None);
            if self.session.identity() != identity
                || self
                    .nav
                    .routes()
                    .with_untracked(|routes| routes.last().cloned())
                    != self.origin.get()
            {
                return;
            }
            match outcome {
                Ok((id, episode)) => self.nav.push(Route::Player(id, episode)),
                Err(error) => self.notice.set(error.text()),
            }
        });
        self.task.with(|slot| *slot = Some(task));
    }
    fn cancel(self) {
        if let Some(task) = self.task.with(Option::take) {
            task.cancel();
        }
        self.pending.set(None);
    }
}
