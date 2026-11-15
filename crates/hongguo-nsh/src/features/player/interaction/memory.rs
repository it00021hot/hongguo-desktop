use super::model::{Action, Overrides, Snapshot, Target, Toggle};
use nsh_core::prelude::*;

/// Session-owned successful interactions. No backend state or global singleton.
#[derive(Clone, Copy)]
pub(crate) struct SocialMemory {
    values: Stored<Overrides>,
    revision: Signal<u64>,
}

impl SocialMemory {
    pub(crate) fn new() -> Self {
        Self {
            values: stored(Overrides::default()),
            revision: signal(0),
        }
    }
    pub(super) fn refresh(
        self,
        account: &str,
        target: &Target,
        server: Snapshot,
        current: Snapshot,
    ) -> Snapshot {
        self.revision.get();
        let mut next = self
            .values
            .with(|values| values.apply(account, target, server));
        // Another screen's completion must not clear this screen's in-flight mutation.
        if current.like.pending {
            next.like = current.like;
        }
        if current.collect.pending {
            next.collect = current.collect;
        }
        if current.follow.pending {
            next.follow = current.follow;
        }
        next
    }
    pub(super) fn remember(self, account: &str, target: &Target, action: Action, value: Toggle) {
        self.values
            .with(|values| values.remember(account, target, action, value));
        self.revision.update(|revision| *revision += 1);
    }
}
