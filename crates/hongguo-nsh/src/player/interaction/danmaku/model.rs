use hongguo_api::danmaku::{Danmaku, DanmakuWindowPage};

pub(super) const WINDOW_MS: u64 = 30_000;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Request {
    pub(super) series: String,
    pub(super) vid: String,
    pub(super) start: u64,
    pub(super) duration: u64,
    pub(super) cursor: String,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct Timeline {
    pub(super) series: String,
    pub(super) vid: String,
    pub(super) items: Vec<Danmaku>,
    last: Option<Request>,
    next_start: u64,
    next_cursor: String,
    has_more: bool,
}

impl Timeline {
    pub(super) fn new(series: String, vid: String) -> Self {
        Self {
            series,
            vid,
            ..Default::default()
        }
    }
    pub(super) fn request(&self, at: u64, duration: u64) -> Option<Request> {
        if self.vid.is_empty() || duration == 0 || at >= duration {
            return None;
        }
        let (start, cursor) = match &self.last {
            None => (at, String::new()),
            Some(last)
                if at < last.start
                    || at
                        >= self
                            .next_start
                            .max(last.start + WINDOW_MS)
                            .saturating_add(WINDOW_MS) =>
            {
                (at, String::new())
            }
            Some(last)
                if !self.next_cursor.is_empty()
                    && self.next_start > last.start
                    && at >= self.next_start =>
            {
                (self.next_start, self.next_cursor.clone())
            }
            Some(last)
                if self.has_more
                    && self.next_start <= last.start
                    && !self.next_cursor.is_empty()
                    && self.next_cursor != last.cursor =>
            {
                (last.start, self.next_cursor.clone())
            }
            _ => return None,
        };
        Some(Request {
            series: self.series.clone(),
            vid: self.vid.clone(),
            start,
            duration,
            cursor,
        })
    }
    pub(super) fn accept(&mut self, request: Request, page: DanmakuWindowPage) {
        if request.series != self.series || request.vid != self.vid {
            return;
        }
        let low = request.start.saturating_sub(WINDOW_MS);
        let high = request.start.saturating_add(WINDOW_MS * 2);
        self.items
            .retain(|item| item.offset_ms >= low && item.offset_ms <= high);
        for item in page.items {
            self.insert(item);
        }
        self.next_start = page.next_start_ms;
        self.next_cursor = page.next_cursor;
        self.has_more = page.has_more;
        self.last = Some(request);
    }
    pub(super) fn insert(&mut self, item: Danmaku) {
        if !self
            .items
            .iter()
            .any(|old| old.comment_id == item.comment_id)
        {
            self.items.push(item);
            self.items.sort_by_key(|item| item.offset_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(next: u64, cursor: &str, has_more: bool) -> DanmakuWindowPage {
        DanmakuWindowPage {
            items: vec![],
            next_start_ms: next,
            next_cursor: cursor.into(),
            has_more,
        }
    }
    #[test]
    fn false_has_more_still_advances_with_the_exact_server_cursor() {
        let mut timeline = Timeline::new("s".into(), "v".into());
        let first = timeline.request(0, 199000).unwrap();
        timeline.accept(first, page(30000, "opaque server cursor", false));
        assert!(timeline.request(29999, 199000).is_none());
        let next = timeline.request(30000, 199000).unwrap();
        assert_eq!(next.start, 30000);
        assert_eq!(next.cursor, "opaque server cursor");
        assert_eq!(next.duration, 199000);
    }
    #[test]
    fn seeks_restart_at_the_playhead_and_repeated_cursors_stop_pagination() {
        let mut timeline = Timeline::new("s".into(), "v".into());
        let first = timeline.request(60000, 199000).unwrap();
        timeline.accept(first, page(90000, "cursor", false));
        let backward = timeline.request(10000, 199000).unwrap();
        assert_eq!(backward.start, 10000);
        assert!(backward.cursor.is_empty());
        let forward = timeline.request(150000, 199000).unwrap();
        assert_eq!(forward.start, 150000);
        assert!(forward.cursor.is_empty());
        let same = Request {
            series: "s".into(),
            vid: "v".into(),
            start: 60000,
            duration: 199000,
            cursor: "cursor".into(),
        };
        timeline.accept(same, page(60000, "cursor", true));
        assert!(timeline.request(61000, 199000).is_none());
    }
    #[test]
    fn old_episode_responses_are_ignored_and_sent_echoes_are_deduplicated() {
        let mut timeline = Timeline::new("s".into(), "v".into());
        let mut wrong = timeline.request(0, 20000).unwrap();
        wrong.vid = "old".into();
        timeline.accept(wrong, page(10000, "wrong", false));
        assert!(timeline.last.is_none());
        let item = Danmaku {
            comment_id: "new".into(),
            text: "Hello".into(),
            offset_ms: 3000,
            digg_count: 0,
        };
        timeline.insert(item.clone());
        timeline.insert(item);
        assert_eq!(timeline.items.len(), 1);
        assert_eq!(timeline.items[0].comment_id, "new");
    }
}
