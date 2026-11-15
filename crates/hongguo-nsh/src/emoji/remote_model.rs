use hongguo_api::emoticon::{Emoticon, EmoticonPage};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    System,
    ThirdParty,
    Own,
    Search,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Query {
    pub tab: Tab,
    pub keyword: String,
    pub offset: i64,
    pub revision: u64,
    pub account: Option<String>,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct Feed {
    pub query: Query,
    pub items: Vec<Emoticon>,
    pub has_more: bool,
    next: i64,
    accepted: Option<Query>,
}

impl Feed {
    pub fn select(&mut self, tab: Tab, keyword: String) {
        self.query.tab = tab;
        self.query.keyword = keyword.trim().to_owned();
        self.reset();
    }
    pub fn reset(&mut self) {
        self.query.offset = 0;
        self.query.revision += 1;
        self.items.clear();
        self.has_more = false;
        self.next = 0;
        self.accepted = None;
    }
    pub fn more(&mut self) {
        if self.has_more {
            self.query.offset = self.next;
        }
    }
    pub fn accepts_load(&self) -> bool {
        match self.query.tab {
            Tab::System => false,
            Tab::Search => !self.query.keyword.is_empty(),
            Tab::Own => self.query.account.is_some(),
            Tab::ThirdParty => true,
        }
    }
    pub fn accept(&mut self, query: &Query, page: EmoticonPage) -> bool {
        if query != &self.query || self.accepted.as_ref() == Some(query) {
            return false;
        }
        if query.offset == 0 {
            self.items.clear();
        }
        for item in page.items {
            if !self
                .items
                .iter()
                .any(|existing| key(existing) == key(&item))
            {
                self.items.push(item);
            }
        }
        self.accepted = Some(query.clone());
        self.has_more = page.has_more && page.next_offset > query.offset;
        self.next = page.next_offset;
        true
    }
}

pub(super) fn key(item: &Emoticon) -> &str {
    item.id
        .as_deref()
        .filter(|id| !id.is_empty())
        .unwrap_or(&item.web_uri)
}
