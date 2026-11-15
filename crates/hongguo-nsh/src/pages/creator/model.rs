use hongguo_api::danmaku::{CreatorWork, CreatorWorksPage};
use std::collections::HashSet;

#[derive(Clone, Default, PartialEq)]
pub(super) struct Query {
    pub cursor: String,
    pub previous: Vec<CreatorWork>,
    pub first_total: Option<i64>,
    pub identity: u64,
}

impl Query {
    pub fn finish(self, mut page: CreatorWorksPage) -> CreatorWorksPage {
        let mut ids = HashSet::new();
        page.works = self
            .previous
            .into_iter()
            .chain(page.works)
            .filter(|item| !item.series_id.is_empty() && ids.insert(item.series_id.clone()))
            .collect();
        page.total = self.first_total.unwrap_or(page.total);
        page
    }
    pub fn next(&self, page: &CreatorWorksPage, identity: u64) -> Option<Self> {
        if !page.has_more || page.cursor.is_empty() || page.cursor == self.cursor {
            return None;
        }
        Some(Self {
            cursor: page.cursor.clone(),
            previous: page.works.clone(),
            first_total: Some(page.total),
            identity,
        })
    }
}
