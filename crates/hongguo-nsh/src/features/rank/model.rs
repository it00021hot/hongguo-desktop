use hongguo_service::contracts::rank::{
    RankItem, RankPanelItem, RankPanelRow, RankSubList, RankTab,
};

pub(super) const DEFAULT_TAB: &str = "all";
pub(super) const DEFAULT_SUB: &str = "ranklist_hot_sc";

#[derive(Clone, Debug, PartialEq)]
pub(in crate::features) struct ScrollScope {
    tab: String,
    sub: String,
    panel: String,
    background: String,
    identity: u64,
}

#[derive(Clone, PartialEq)]
pub(super) struct Query {
    pub(super) tab: String,
    pub(super) sub: String,
    pub(super) panel: String,
    pub(super) background: String,
    pub(super) offset: i64,
    pub(super) session: String,
    pub(super) version: String,
    pub(super) previous: Vec<RankItem>,
    pub(super) identity: u64,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            tab: DEFAULT_TAB.into(),
            sub: DEFAULT_SUB.into(),
            panel: String::new(),
            background: String::new(),
            offset: 0,
            session: String::new(),
            version: String::new(),
            previous: Vec::new(),
            identity: 0,
        }
    }
}

impl Query {
    pub(super) fn scroll_scope(&self) -> ScrollScope {
        ScrollScope {
            tab: self.tab.clone(),
            sub: self.sub.clone(),
            panel: self.panel.clone(),
            background: self.background.clone(),
            identity: self.identity,
        }
    }

    pub(super) fn view_snapshot(&self) -> Self {
        Self {
            tab: self.tab.clone(),
            sub: self.sub.clone(),
            panel: self.panel.clone(),
            background: self.background.clone(),
            ..Default::default()
        }
    }

    pub(super) fn reset_paging(&mut self) {
        self.offset = 0;
        self.session.clear();
        self.version.clear();
        self.previous.clear();
    }
}

pub(super) fn normalize(tabs: Vec<RankTab>, all: &str, filter: &str) -> Vec<RankTab> {
    if tabs.iter().any(|tab| tab.id == DEFAULT_TAB) {
        return tabs
            .into_iter()
            .filter(|tab| tab.id != "ranklist_celebrity" && !tab.subs.is_empty())
            .collect();
    }
    if tabs.is_empty() {
        return Vec::new();
    }
    vec![RankTab {
        id: DEFAULT_TAB.into(),
        name: all.into(),
        subs: tabs
            .into_iter()
            .map(|tab| RankSubList {
                id: tab.id,
                name: tab.name,
                description: String::new(),
                background: Vec::new(),
                panel: if tab.subs.is_empty() {
                    Vec::new()
                } else {
                    vec![RankPanelRow {
                        name: filter.into(),
                        items: tab
                            .subs
                            .into_iter()
                            .map(|sub| RankPanelItem {
                                id: sub.id,
                                name: sub.name,
                            })
                            .collect(),
                    }]
                },
            })
            .collect(),
    }]
}
