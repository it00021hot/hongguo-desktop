use hongguo_api::discover::{FeedItem, FeedPage};

pub(super) const DEFAULT_CHANNEL: &str = "16";
pub(super) const FOLLOW_CHANNEL: &str = "45";
pub(super) const CHANNELS: [(&str, &str); 3] = [
    ("16", "home.tabFeed"),
    ("36", "home.tabComic"),
    ("39", "home.tabHuman"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::start_episode;
    use hongguo_common::model::{Episode, Series};

    fn item(id: &str, title: &str) -> FeedItem {
        FeedItem {
            series_id: id.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn relayed_season_joins_the_current_feed_without_losing_its_next_recommendation() {
        let mut page = Channel {
            items: vec![
                item("one", "one"),
                item("two", "two"),
                item("three", "three"),
            ],
            index: 1,
            ..Default::default()
        };
        page.goto(Series {
            series_id: "season".into(),
            title: "Second season".into(),
            episode_count: 24,
            ..Default::default()
        });
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.series_id.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "season", "three"]
        );
        assert_eq!(page.items[page.index].series_id, "season");
        assert!(page.step(1));
        assert_eq!(page.items[page.index].series_id, "three");
        page.goto(Series {
            series_id: "one".into(),
            ..Default::default()
        });
        assert_eq!(page.index, 0);
        assert_eq!(page.items.len(), 4);
    }

    #[test]
    fn later_pages_update_duplicates_without_moving_the_playing_drama() {
        let mut channel = Channel {
            items: vec![item("one", "one"), item("two", "old")],
            index: 1,
            ..Default::default()
        };
        channel.merge(FeedPage {
            items: vec![item("two", "new"), item("three", "third")],
            next_offset: 6,
            has_more: true,
            session_id: "server-session".into(),
        });
        assert_eq!(
            channel
                .items
                .iter()
                .map(|item| item.series_id.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
        assert_eq!(channel.items[channel.index].title, "new");
        assert_eq!(
            (channel.offset, channel.session.as_str(), channel.has_more),
            (6, "server-session", true)
        );
    }

    #[test]
    fn feed_navigation_clamps_both_edges_and_an_empty_feed() {
        let mut channel = Channel::default();
        assert!(!channel.step(1));
        channel.items = vec![item("one", "one"), item("two", "two")];
        assert!(!channel.step(-1));
        assert!(channel.step(1));
        assert_eq!(channel.index, 1);
        assert!(!channel.step(1));
        assert!(channel.step(-1));
        assert_eq!(channel.index, 0);
    }

    #[test]
    fn startup_resumes_a_real_episode_and_never_invents_episode_one() {
        let series = Series {
            episodes: vec![
                Episode {
                    vid_index: 3,
                    ..Default::default()
                },
                Episode {
                    vid_index: 9,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(start_episode(&series, Some(9)).unwrap(), 9);
        assert_eq!(start_episode(&series, Some(1)).unwrap(), 3);
        assert_eq!(start_episode(&series, None).unwrap(), 3);
        assert!(start_episode(&Series::default(), None).is_err());
    }
}
