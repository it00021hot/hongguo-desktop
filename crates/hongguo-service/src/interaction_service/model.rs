use hongguo_common::model::{Episode, Series};

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub series_id: String,
    pub vid: String,
    pub title: String,
    pub likes: i64,
    pub collections: i64,
    pub comments: i64,
}

impl Target {
    pub fn from_episode(series: &Series, episode: &Episode) -> Self {
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
pub enum Action {
    Like,
    Collect,
    Follow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Toggle {
    pub selected: bool,
    pub count: i64,
    pub pending: bool,
}

impl Toggle {
    pub fn new(selected: bool, count: i64) -> Self {
        Self {
            selected,
            count: count.max(0),
            pending: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub like: Toggle,
    pub collect: Toggle,
    pub follow: Toggle,
    pub show_follow: bool,
}

impl Snapshot {
    pub fn value(self, action: Action) -> Toggle {
        match action {
            Action::Like => self.like,
            Action::Collect => self.collect,
            Action::Follow => self.follow,
        }
    }

    pub fn value_mut(&mut self, action: Action) -> &mut Toggle {
        match action {
            Action::Like => &mut self.like,
            Action::Collect => &mut self.collect,
            Action::Follow => &mut self.follow,
        }
    }

    pub fn begin(&mut self, action: Action) -> Option<Toggle> {
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

    pub fn finish(&mut self, action: Action, before: Toggle, success: bool) {
        if success {
            self.value_mut(action).pending = false;
        } else {
            *self.value_mut(action) = before;
        }
    }
}
