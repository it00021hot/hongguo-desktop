use crate::{
    bridge,
    features::SessionHandle,
    navigation::{Navigation, Route},
};
use nsh_core::prelude::*;

enum Entry {
    Regular,
    Linked,
}

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
    let origin = nav.routes().get_untracked().last().cloned();
    start_action_for(session, nav, origin)
}

pub(super) fn start_action_for(
    session: SessionHandle,
    nav: Navigation,
    origin: Option<Route>,
) -> StartAction {
    let action = StartAction {
        session,
        nav,
        origin: stored(origin),
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
    pub fn notice(self) -> Signal<SharedString> {
        self.notice
    }
    pub fn start(self, id: String) {
        self.request(id, Entry::Regular);
    }
    pub(super) fn start_linked(self, id: String) {
        self.request(id, Entry::Linked);
    }
    fn request(self, id: String, entry: Entry) {
        if self.pending.get_untracked().as_ref() == Some(&id) {
            return;
        }
        self.cancel();
        self.notice.set(SharedString::default());
        self.pending.set(Some(id.clone()));
        let state = self.session.with(|session| session.clone());
        let identity = self.session.identity();
        let contextual = !matches!(&entry, Entry::Regular);
        let task = spawn(async move {
            let outcome = bridge::request(async move {
                match entry {
                    Entry::Regular => super::resolve_start(state, id).await,
                    Entry::Linked => super::resolve_feed_start(state, id, None, None).await,
                }
            })
            .await;
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
                Err(_) if contextual => self.notice.set(crate::i18n::tr("player.linkedLoadFailed")),
                Err(error) => self.notice.set(error.text()),
            }
        });
        self.task.with(|slot| *slot = Some(task));
    }
    pub(super) fn cancel(self) {
        if let Some(task) = self.task.with(Option::take) {
            task.cancel();
        }
        self.pending.set(None);
        self.notice.set(SharedString::default());
    }
}
