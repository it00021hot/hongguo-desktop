use hongguo_common::model::{Episode, Series};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::player) struct Target {
    pub(in crate::player) series_id: String,
    pub(in crate::player) vid: String,
    pub(in crate::player) title: String,
    pub(in crate::player) likes: i64,
    pub(in crate::player) collections: i64,
    pub(in crate::player) comments: i64,
}

impl Target {
    pub(in crate::player) fn from_episode(series: &Series, episode: &Episode) -> Self {
        Self {
            series_id: series.series_id.clone(),
            vid: episode.vid.clone(),
            title: series.title.clone(),
            likes: episode.digg_count,
            collections: series.followed_cnt,
            comments: episode.comment_count,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Action {
    Like,
    Collect,
    Follow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Toggle {
    pub(super) selected: bool,
    pub(super) count: i64,
    pub(super) pending: bool,
}

impl Toggle {
    pub(super) fn new(selected: bool, count: i64) -> Self {
        Self {
            selected,
            count: count.max(0),
            pending: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Snapshot {
    pub(super) like: Toggle,
    pub(super) collect: Toggle,
    pub(super) follow: Toggle,
    pub(super) show_follow: bool,
}

impl Snapshot {
    pub(super) fn value(self, action: Action) -> Toggle {
        match action {
            Action::Like => self.like,
            Action::Collect => self.collect,
            Action::Follow => self.follow,
        }
    }

    fn value_mut(&mut self, action: Action) -> &mut Toggle {
        match action {
            Action::Like => &mut self.like,
            Action::Collect => &mut self.collect,
            Action::Follow => &mut self.follow,
        }
    }

    pub(super) fn begin(&mut self, action: Action) -> Option<Toggle> {
        let value = self.value_mut(action);
        if value.pending {
            return None;
        }
        let before = *value;
        value.selected = !value.selected;
        value.count = if value.selected {
            value.count.saturating_add(1)
        } else {
            value.count.saturating_sub(1).max(0)
        };
        value.pending = true;
        Some(before)
    }

    pub(super) fn finish(&mut self, action: Action, before: Toggle, success: bool) {
        if success {
            self.value_mut(action).pending = false;
        } else {
            *self.value_mut(action) = before;
        }
    }
}

#[derive(Default)]
pub(super) struct Overrides(HashMap<(String, Action, String), Toggle>);

impl Overrides {
    fn key(account: &str, target: &Target, action: Action) -> (String, Action, String) {
        (
            account.to_owned(),
            action,
            if action == Action::Like {
                target.vid.clone()
            } else {
                target.series_id.clone()
            },
        )
    }

    pub(super) fn apply(&self, account: &str, target: &Target, mut snapshot: Snapshot) -> Snapshot {
        for action in [Action::Like, Action::Collect, Action::Follow] {
            if let Some(value) = self.0.get(&Self::key(account, target, action)) {
                *snapshot.value_mut(action) = *value;
            }
        }
        snapshot
    }

    pub(super) fn remember(
        &mut self,
        account: &str,
        target: &Target,
        action: Action,
        value: Toggle,
    ) {
        self.0.insert(Self::key(account, target, action), value);
    }
}
