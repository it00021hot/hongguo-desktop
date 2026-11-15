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
