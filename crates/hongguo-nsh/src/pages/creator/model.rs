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

#[cfg(test)]
mod tests {
    use super::*;

    fn work(id: &str) -> CreatorWork {
        CreatorWork {
            series_id: id.into(),
            title: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn pagination_carries_the_exact_cursor_and_preserves_the_first_total() {
        let first = CreatorWorksPage {
            works: vec![work("a"), work("b")],
            cursor: "opaque%2F+cursor".into(),
            has_more: true,
            total: 9,
        };
        let query = Query::default().next(&first, 3).unwrap();
        assert_eq!(query.cursor, first.cursor);
        let final_page = query.finish(CreatorWorksPage {
            works: vec![work("b"), work("c")],
            total: 0,
            ..Default::default()
        });
        assert_eq!(
            final_page
                .works
                .iter()
                .map(|work| work.series_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(final_page.total, 9);
        assert!(Query::default().next(&final_page, 3).is_none());
    }

    #[test]
    fn a_missing_or_unchanged_cursor_cannot_repeat_the_same_page_forever() {
        let query = Query {
            cursor: "same".into(),
            ..Default::default()
        };
        for cursor in ["", "same"] {
            assert!(
                query
                    .next(
                        &CreatorWorksPage {
                            cursor: cursor.into(),
                            has_more: true,
                            ..Default::default()
                        },
                        0
                    )
                    .is_none()
            );
        }
    }
}
