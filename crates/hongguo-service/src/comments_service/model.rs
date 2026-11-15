use hongguo_api::danmaku::{CommentItem, ReplyItem};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Comment(CommentItem),
    Reply(ReplyItem),
}

impl Item {
    pub fn id(&self) -> &str {
        match self {
            Self::Comment(item) => &item.comment_id,
            Self::Reply(item) => &item.reply_id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Comment(item) => &item.user_name,
            Self::Reply(item) => &item.user_name,
        }
    }
    pub fn user_id(&self) -> &str {
        match self {
            Self::Comment(item) => &item.user_id,
            Self::Reply(item) => &item.user_id,
        }
    }
    pub fn images(&self) -> Vec<hongguo_api::emoticon::Emoticon> {
        match self {
            Self::Comment(item) => item.images.clone(),
            Self::Reply(_) => Vec::new(),
        }
    }
    pub fn avatar(&self) -> &str {
        match self {
            Self::Comment(item) => &item.avatar,
            Self::Reply(item) => &item.avatar,
        }
    }
    pub fn text(&self) -> &str {
        match self {
            Self::Comment(item) => &item.text,
            Self::Reply(item) => &item.text,
        }
    }
    pub fn like(&self) -> (bool, i64) {
        match self {
            Self::Comment(item) => (item.user_digg, item.digg_count),
            Self::Reply(item) => (item.user_digg, item.digg_count),
        }
    }
    fn set_like(&mut self, value: (bool, i64)) {
        match self {
            Self::Comment(item) => {
                item.user_digg = value.0;
                item.digg_count = value.1;
            }
            Self::Reply(item) => {
                item.user_digg = value.0;
                item.digg_count = value.1;
            }
        }
    }
}

#[derive(Clone)]
pub struct Page {
    pub summary: Option<super::review_model::ReviewSummary>,
    pub items: Vec<Item>,
    pub total: i64,
    pub cursor: String,
    pub has_more: bool,
}

#[derive(Clone, Default, PartialEq)]
pub struct Feed {
    pub summary: Option<super::review_model::ReviewSummary>,
    pub items: Vec<Item>,
    pub total: i64,
    pub cursor: String,
    pub has_more: bool,
    // Retain send echoes until the eventually consistent list index includes them.
    sent: HashSet<String>,
}

impl Feed {
    pub fn append(&mut self, page: Page, requested: &str) {
        if let Some(summary) = page.summary {
            self.summary = Some(summary);
        }
        for item in page.items {
            self.sent.remove(item.id());
            if !self.items.iter().any(|old| old.id() == item.id()) {
                self.items.push(item);
            }
        }
        self.total = (page.total.max(0) + self.sent.len() as i64).max(self.items.len() as i64);
        self.has_more = page.has_more && !page.cursor.is_empty() && page.cursor != requested;
        self.cursor = page.cursor;
    }
    pub fn accept_sent(&mut self, item: Item) {
        if self.items.iter().any(|old| old.id() == item.id()) {
            return;
        }
        self.sent.insert(item.id().to_owned());
        self.items.insert(0, item);
        self.total = self.total.saturating_add(1);
    }
    pub fn toggle_like(&mut self, id: &str) -> Option<(bool, i64)> {
        let item = self.items.iter_mut().find(|item| item.id() == id)?;
        let before = item.like();
        item.set_like((
            !before.0,
            if before.0 {
                before.1.saturating_sub(1).max(0)
            } else {
                before.1.saturating_add(1)
            },
        ));
        Some(before)
    }
    pub fn restore_like(&mut self, id: &str, before: (bool, i64)) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id() == id) {
            item.set_like(before);
        }
    }
    pub fn add_reply(&mut self, id: &str) {
        if let Some(Item::Comment(item)) = self.items.iter_mut().find(|item| item.id() == id) {
            item.reply_count = item.reply_count.saturating_add(1);
        }
    }
    pub fn remove(&mut self, id: &str) {
        let before = self.items.len();
        self.items.retain(|item| item.id() != id);
        if before != self.items.len() {
            self.total = self.total.saturating_sub(1).max(0);
            self.sent.remove(id);
        }
    }
    pub fn remove_reply(&mut self, id: &str) {
        if let Some(Item::Comment(item)) = self.items.iter_mut().find(|item| item.id() == id) {
            item.reply_count = item.reply_count.saturating_sub(1).max(0);
        }
    }
}
