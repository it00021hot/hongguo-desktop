#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Entry {
    pub(super) key: String,
    pub(super) series_id: String,
    pub(super) title: String,
    pub(super) cover: String,
    pub(super) summary: String,
    pub(super) liked_vid: Option<String>,
    pub(super) can_play: bool,
    pub(super) episode_count: i64,
    pub(super) watched_episode: i64,
    pub(super) finished: Option<bool>,
    pub(super) content_type: i64,
    pub(super) digged_count: i64,
    pub(super) reservation: Option<hongguo_api::calendar::CalendarItem>,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct Query {
    pub(super) identity: Option<u64>,
    pub(super) cursor: String,
    pub(super) offset: i64,
    pub(super) session: String,
    pub(super) online: bool,
    pub(super) previous: Vec<Entry>,
}

#[derive(Clone, Default)]
pub(super) struct Page {
    pub(super) source: Option<PageSource>,
    pub(super) items: Vec<Entry>,
    pub(super) cursor: String,
    pub(super) offset: i64,
    pub(super) session: String,
    pub(super) has_more: bool,
    pub(super) warning: Option<crate::bridge::Failure>,
    pub(super) reservation_counts: Option<(i64, i64)>,
}

#[derive(Clone, PartialEq)]
pub(super) struct PageSource {
    pub(super) identity: u64,
    pub(super) online: bool,
    cursor: String,
    offset: i64,
    session: String,
}

impl Query {
    pub(super) fn for_identity(&self, identity: u64) -> Self {
        if self.identity == Some(identity) {
            self.clone()
        } else {
            Self {
                identity: Some(identity),
                online: self.online,
                ..Default::default()
            }
        }
    }

    pub(super) fn source(&self, identity: u64) -> PageSource {
        let query = self.for_identity(identity);
        PageSource {
            identity,
            online: query.online,
            cursor: query.cursor,
            offset: query.offset,
            session: query.session,
        }
    }
}

impl Page {
    pub(super) fn belongs_to(&self, query: &Query, identity: u64) -> bool {
        self.source
            .as_ref()
            .is_some_and(|source| source.identity == identity && source.online == query.online)
    }

    pub(super) fn next_query(&self, query: &Query, identity: u64) -> Option<Query> {
        let query = query.for_identity(identity);
        (self.has_more
            && self.source.as_ref() == Some(&query.source(identity))
            && (self.offset > query.offset
                || (!self.cursor.is_empty() && self.cursor != query.cursor)))
            .then(|| Query {
                identity: Some(identity),
                cursor: self.cursor.clone(),
                offset: self.offset,
                session: self.session.clone(),
                online: query.online,
                previous: self.items.clone(),
            })
    }
}

pub(super) fn merge_page(mut entries: Vec<Entry>, incoming: Vec<Entry>) -> Vec<Entry> {
    let mut positions: std::collections::HashMap<String, usize> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.key.clone(), index))
        .collect();
    for entry in incoming {
        if let Some(index) = positions.get(&entry.key) {
            entries[*index] = entry;
        } else {
            positions.insert(entry.key.clone(), entries.len());
            entries.push(entry);
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, title: &str) -> Entry {
        Entry {
            key: id.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn an_empty_page_preserves_existing_entries() {
        assert!(merge_page(Vec::new(), Vec::new()).is_empty());
        let first = vec![entry("one", "first")];
        assert_eq!(merge_page(first.clone(), Vec::new()), first);
    }

    #[test]
    fn repeated_ids_update_in_place_without_duplicate_rows() {
        let merged = merge_page(
            vec![entry("one", "old"), entry("two", "second")],
            vec![
                entry("two", "updated"),
                entry("three", "third"),
                entry("three", "latest"),
            ],
        );
        assert_eq!(
            merged,
            vec![
                entry("one", "old"),
                entry("two", "updated"),
                entry("three", "latest")
            ]
        );
    }

    #[test]
    fn pagination_rejects_stale_account_tab_request_and_stalled_cursors() {
        let query = Query {
            online: true,
            identity: Some(7),
            offset: 10,
            ..Default::default()
        };
        let mut page = Page {
            source: Some(query.source(7)),
            offset: 20,
            has_more: true,
            ..Default::default()
        };
        assert_eq!(page.next_query(&query, 7).unwrap().offset, 20);
        assert!(page.next_query(&query, 8).is_none());
        assert!(
            page.next_query(
                &Query {
                    online: false,
                    ..query.clone()
                },
                7
            )
            .is_none()
        );
        assert!(
            page.next_query(
                &Query {
                    offset: 20,
                    ..query.clone()
                },
                7
            )
            .is_none()
        );
        page.offset = query.offset;
        assert!(page.next_query(&query, 7).is_none());
        page.cursor = "next".into();
        assert_eq!(page.next_query(&query, 7).unwrap().cursor, "next");
    }
}
