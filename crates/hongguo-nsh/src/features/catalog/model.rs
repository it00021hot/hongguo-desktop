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
