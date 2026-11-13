//! End-of-series relay. All async work is source/account owned and drops when
//! its request is invalidated; normal same-series episode changes stay outside.
mod backend;
mod model;
mod related;
#[cfg(test)]
mod tests;
mod workflow;

pub(super) use model::SourceKey;
pub(super) use related::Related;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SourceEpoch {
    pub key: SourceKey,
    pub feed_revision: u64,
}

use super::navigation::FeedNavigation;
use crate::{events::SessionHandle, i18n::tr};
use hongguo_common::model::Series;
use model::{Gate, Ticket};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct State {
    pub active: Memo<bool>,
    pub downloading: Memo<bool>,
    pub source: Memo<Option<SourceKey>>,
    pub paused: Signal<bool>,
}

#[derive(Clone, Copy)]
pub(super) struct Controller {
    pub related: Related,
    state: State,
    identity: Memo<u64>,
    gate: Stored<Gate>,
    requested: Signal<Option<Ticket>>,
    feed_revision: Memo<u64>,
}

pub(super) fn observe(
    session: SessionHandle,
    selection: Signal<(String, u32)>,
    state: State,
    feed: Option<FeedNavigation>,
    notice: Signal<SharedString>,
    selected: impl Fn(Series, u32) + Copy + 'static,
) -> Controller {
    let related = related::observe(session, selection, state.active);
    let controller = Controller::build(
        state,
        computed(move || session.identity()),
        related,
        computed(move || feed.map_or(0, |feed| feed.revision())),
    );
    workflow::install(
        controller,
        feed,
        notice,
        selected,
        backend::resolver(session.with(|session| session.clone())),
    );
    controller
}

impl Controller {
    fn build(
        state: State,
        identity: Memo<u64>,
        related: Related,
        feed_revision: Memo<u64>,
    ) -> Self {
        let controller = Self {
            state,
            identity,
            related,
            gate: stored(Gate::default()),
            requested: signal(None),
            feed_revision,
        };
        effect(move || {
            let current = if state.active.get() && !state.downloading.get() {
                state.source.get().map(|source| (source, identity.get()))
            } else {
                None
            };
            controller.gate.with(|gate| gate.observe(current));
            if let Some(request) = controller.requested.get()
                && !controller.current(&request)
            {
                controller.gate.with(|gate| gate.finish(&request, false));
                controller.requested.set(None);
            }
        });
        controller
    }

    fn current(self, ticket: &Ticket) -> bool {
        self.state.active.get()
            && !self.state.downloading.get()
            && self.state.source.get().as_ref() == Some(&ticket.source)
            && self.identity.get() == ticket.identity
            && self.state.paused.get() == ticket.paused
            && self.feed_revision.get() == ticket.feed_revision
            && self.gate.with(|gate| gate.matches(ticket))
    }

    pub fn advance(self) -> bool {
        if !self.state.active.get_untracked() || self.state.downloading.get_untracked() {
            return false;
        }
        let Some(source) = self.state.source.get_untracked() else {
            return false;
        };
        if self
            .requested
            .get_untracked()
            .as_ref()
            .is_some_and(|ticket| self.current(ticket))
            || self
                .gate
                .with(|gate| gate.completed(&source, self.identity.get_untracked()))
        {
            return true;
        }
        let Some(ticket) = self.gate.with(|gate| {
            gate.begin(
                source,
                self.identity.get_untracked(),
                self.state.paused.get_untracked(),
                self.feed_revision.get_untracked(),
            )
        }) else {
            return false;
        };
        self.requested.set(Some(ticket));
        true
    }

    pub fn pending(self) -> Memo<bool> {
        computed(move || self.requested.get().is_some())
    }

    pub fn source_key(self) -> Memo<Option<SourceEpoch>> {
        computed(move || {
            self.state
                .active
                .get()
                .then(|| {
                    self.state.source.get().map(|key| SourceEpoch {
                        key,
                        feed_revision: self.feed_revision.get(),
                    })
                })
                .flatten()
        })
    }
}

fn last_notice(episode: u32) -> SharedString {
    tr("player.lastEpisode")
        .replace("{index}", &episode.to_string())
        .into()
}
