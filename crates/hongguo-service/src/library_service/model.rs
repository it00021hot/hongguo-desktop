#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub key: String,
    pub series_id: String,
    pub title: String,
    pub cover: String,
    pub summary: String,
    pub liked_vid: Option<String>,
    pub collection_item_id: Option<String>,
    pub collection_folder_id: Option<i64>,
    pub collection_book_type: Option<i64>,
    pub collection_book_id: Option<String>,
    pub can_play: bool,
    pub episode_count: i64,
    pub watched_episode: i64,
    pub finished: Option<bool>,
    pub content_type: i64,
    pub digged_count: i64,
    pub reservation: Option<hongguo_api::calendar::CalendarItem>,
}

#[derive(Clone, Default, PartialEq)]
pub struct Query {
    pub identity: Option<u64>,
    pub cursor: String,
    pub folder_id: Option<i64>,
    pub offset: i64,
    pub session: String,
    pub online: bool,
    pub previous: Vec<Entry>,
}

#[derive(Clone, Default)]
pub struct Page {
    pub source: Option<PageSource>,
    pub items: Vec<Entry>,
    pub folder: Option<hongguo_api::interact::BookshelfFolder>,
    pub cursor: String,
    pub offset: i64,
    pub session: String,
    pub has_more: bool,
    pub warning: Option<hongguo_common::AppError>,
    pub reservation_counts: Option<(i64, i64)>,
}

#[derive(Clone, PartialEq)]
pub struct PageSource {
    pub identity: u64,
    pub online: bool,
    cursor: String,
    folder_id: Option<i64>,
    offset: i64,
    session: String,
}

impl Query {
    pub fn for_identity(&self, identity: u64) -> Self {
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

    pub fn source(&self, identity: u64) -> PageSource {
        let query = self.for_identity(identity);
        PageSource {
            identity,
            online: query.online,
            cursor: query.cursor,
            folder_id: query.folder_id,
            offset: query.offset,
            session: query.session,
        }
    }
}

impl Page {
    pub fn belongs_to(&self, query: &Query, identity: u64) -> bool {
        let folder_id = if query.identity == Some(identity) {
            query.folder_id
        } else {
            None
        };
        self.source.as_ref().is_some_and(|source| {
            source.identity == identity
                && source.online == query.online
                && source.folder_id == folder_id
        })
    }

    pub fn next_query(&self, query: &Query, identity: u64) -> Option<Query> {
        let query = query.for_identity(identity);
        (self.has_more
            && self.source.as_ref() == Some(&query.source(identity))
            && (self.offset > query.offset
                || (!self.cursor.is_empty() && self.cursor != query.cursor)))
            .then(|| Query {
                identity: Some(identity),
                cursor: self.cursor.clone(),
                folder_id: query.folder_id,
                offset: self.offset,
                session: self.session.clone(),
                online: query.online,
                previous: self.items.clone(),
            })
    }
}

pub fn merge_page(mut entries: Vec<Entry>, incoming: Vec<Entry>) -> Vec<Entry> {
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
