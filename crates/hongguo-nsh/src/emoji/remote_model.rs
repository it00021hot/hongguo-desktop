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

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str) -> Emoticon {
        Emoticon {
            id: Some(id.into()),
            web_uri: format!("https://test.invalid/{id}"),
            ..Default::default()
        }
    }

    #[test]
    fn pagination_keeps_server_cursors_and_deduplicates_items_without_accepting_stale_searches() {
        let mut feed = Feed::default();
        feed.select(Tab::ThirdParty, String::new());
        let first = feed.query.clone();
        feed.accept(
            &first,
            EmoticonPage {
                items: vec![item("a")],
                has_more: true,
                next_offset: 100,
            },
        );
        feed.more();
        let next = feed.query.clone();
        assert_eq!(next.offset, 100);
        feed.accept(
            &next,
            EmoticonPage {
                items: vec![item("a"), item("b")],
                has_more: true,
                next_offset: 100,
            },
        );
        assert_eq!(feed.items.len(), 2);
        assert!(!feed.has_more);
        feed.select(Tab::Search, " cat ".into());
        assert!(!feed.accept(
            &first,
            EmoticonPage {
                items: vec![item("old")],
                ..Default::default()
            }
        ));
        assert!(feed.items.is_empty());
    }

    #[test]
    fn blank_search_system_and_anonymous_own_tabs_never_start_a_network_request() {
        let mut feed = Feed::default();
        assert!(!feed.accepts_load());
        feed.select(Tab::Search, " \n ".into());
        assert!(!feed.accepts_load());
        feed.select(Tab::Own, String::new());
        assert!(!feed.accepts_load());
        feed.query.account = Some("account".into());
        assert!(feed.accepts_load());
    }
}
