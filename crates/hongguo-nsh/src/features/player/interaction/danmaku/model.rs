use hongguo_service::contracts::danmaku::{Danmaku, DanmakuWindowPage};

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
