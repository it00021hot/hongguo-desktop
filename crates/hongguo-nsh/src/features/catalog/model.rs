use hongguo_service::contracts::discover::{FeedItem, FeedPage};

pub(super) const DEFAULT_CHANNEL: &str = "16";
pub(super) const CHANNELS: [(&str, &str); 4] = [
    ("45", "home.tabFollowing"),
    ("36", "home.tabComic"),
    ("39", "home.tabHuman"),
    ("16", "home.tabFeed"),
];
pub(super) const PREFETCH_REMAINING: usize = 3;

#[derive(Clone, Default, PartialEq)]
pub(super) struct Query {
    pub channel: String,
    pub offset: i64,
    pub session: String,
    pub seen: Vec<String>,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct Channel {
    pub items: Vec<FeedItem>,
    pub index: usize,
    pub offset: i64,
    pub session: String,
    pub has_more: bool,
}

impl Channel {
    pub(super) fn goto(&mut self, series: hongguo_common::model::Series) {
        if let Some(index) = self
            .items
            .iter()
            .position(|item| item.series_id == series.series_id)
        {
            self.index = index;
        } else {
            let at = self.index.saturating_add(1).min(self.items.len());
            self.items.insert(
                at,
                FeedItem {
                    series_id: series.series_id,
                    title: series.title,
                    cover: series.cover,
                    episode_cnt: series.episode_count,
                    ..Default::default()
                },
            );
            self.index = at;
        }
    }

    pub(super) fn merge(&mut self, page: FeedPage) {
        for item in page.items {
            if let Some(existing) = self
                .items
                .iter_mut()
                .find(|existing| existing.series_id == item.series_id)
            {
                *existing = item;
            } else {
                self.items.push(item);
            }
        }
        self.offset = page.next_offset;
        self.session = page.session_id;
        self.has_more = page.has_more;
        self.index = self.index.min(self.items.len().saturating_sub(1));
    }
    pub(super) fn step(&mut self, direction: i32) -> bool {
        let next = if direction < 0 {
            self.index.saturating_sub(1)
        } else if direction > 0 {
            (self.index + 1).min(self.items.len().saturating_sub(1))
        } else {
            self.index
        };
        let changed = next != self.index;
        self.index = next;
        changed
    }
}

pub(super) fn retain_playable(
    previous: Option<(String, u32)>,
    ready: Option<(String, u32)>,
) -> Option<(String, u32)> {
    ready.or(previous)
}

pub(super) fn cold_loading(
    has_playable: bool,
    target_loading: bool,
    feed_loading: bool,
    has_item: bool,
) -> bool {
    !has_playable && (target_loading || (feed_loading && !has_item))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retain_playable_keeps_previous_while_next_target_loads() {
        assert_eq!(
            retain_playable(Some(("a".into(), 1)), None),
            Some(("a".into(), 1))
        );
        assert_eq!(
            retain_playable(Some(("a".into(), 1)), Some(("b".into(), 1))),
            Some(("b".into(), 1))
        );
        assert_eq!(
            retain_playable(None, Some(("b".into(), 2))),
            Some(("b".into(), 2))
        );
    }

    #[test]
    fn cold_loading_only_before_a_player_surface_exists() {
        assert!(!cold_loading(true, true, false, true));
        assert!(cold_loading(false, true, false, true));
        assert!(cold_loading(false, false, true, false));
        assert!(!cold_loading(false, false, false, false));
    }
}
