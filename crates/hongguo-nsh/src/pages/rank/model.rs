use hongguo_api::rank::{RankItem, RankPanelItem, RankPanelRow, RankSubList, RankTab};

pub(super) const DEFAULT_TAB: &str = "all";
pub(super) const DEFAULT_SUB: &str = "ranklist_hot_sc";

#[derive(Clone, Debug, PartialEq)]
pub(in crate::pages) struct ScrollScope {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_scope_keeps_pagination_and_payload_out_but_isolates_identity_and_filters() {
        let query = Query::default();
        let scope = query.scroll_scope();
        let mut paged = query.clone();
        paged.offset = 20;
        paged.session = "next-page".into();
        paged.version = "new-version".into();
        paged.previous = vec![RankItem::default()];
        assert_eq!(paged.scroll_scope(), scope);
        for field in 0..5 {
            let mut different = query.clone();
            match field {
                0 => different.tab = "comic".into(),
                1 => different.sub = "recommended".into(),
                2 => different.panel = "modern".into(),
                3 => different.background = "month".into(),
                _ => different.identity = 1,
            }
            assert_ne!(different.scroll_scope(), scope);
        }
    }

    fn sub(id: &str) -> RankSubList {
        RankSubList {
            id: id.into(),
            name: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn current_schema_preserves_filters_and_hides_non_series_tabs() {
        let all = RankTab {
            id: "all".into(),
            name: "All".into(),
            subs: vec![RankSubList {
                panel: vec![RankPanelRow {
                    name: "Era".into(),
                    items: vec![RankPanelItem {
                        id: "modern".into(),
                        name: "Modern".into(),
                    }],
                }],
                ..sub("hot")
            }],
        };
        let result = normalize(
            vec![
                all.clone(),
                RankTab {
                    id: "ranklist_celebrity".into(),
                    subs: vec![sub("actor")],
                    ..Default::default()
                },
                RankTab::default(),
            ],
            "All",
            "Filter",
        );
        assert_eq!(result, vec![all]);
    }

    #[test]
    fn legacy_schema_maps_nested_options_to_a_single_filter_row() {
        let result = normalize(
            vec![RankTab {
                id: "hot".into(),
                name: "Hot".into(),
                subs: vec![sub("modern"), sub("period")],
            }],
            "All",
            "Filter",
        );
        assert_eq!(result[0].id, "all");
        assert_eq!(result[0].subs[0].id, "hot");
        assert_eq!(result[0].subs[0].panel[0].name, "Filter");
        assert_eq!(
            result[0].subs[0].panel[0]
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["modern", "period"]
        );
        assert!(normalize(Vec::new(), "All", "Filter").is_empty());
    }

    #[test]
    fn changing_a_filter_discards_every_pagination_field() {
        let mut query = Query {
            panel: "modern".into(),
            background: "date_202610".into(),
            offset: 20,
            session: "session".into(),
            version: "version".into(),
            previous: vec![RankItem::default()],
            identity: 7,
            ..Default::default()
        };
        query.reset_paging();
        assert_eq!(
            (
                query.offset,
                query.session.as_str(),
                query.version.as_str(),
                query.previous.len()
            ),
            (0, "", "", 0)
        );
        assert_eq!(
            (
                query.panel.as_str(),
                query.background.as_str(),
                query.identity
            ),
            ("modern", "date_202610", 7)
        );
    }
}
