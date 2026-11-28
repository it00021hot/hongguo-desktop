use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

const ROWS_PER_CHUNK: usize = 128;
const INDEX_SHARDS: usize = 128;

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

/// Ordered rows shared with outstanding queries; appending only copies touched chunks and index shards.
#[derive(Clone, Default)]
pub struct EntryRows(Option<Arc<RowsSnapshot>>);

type SharedIndex = Arc<HashMap<Arc<str>, usize>>;

#[derive(Clone)]
struct RowsSnapshot {
    chunks: Vec<Arc<Vec<Arc<Entry>>>>,
    index: [Option<SharedIndex>; INDEX_SHARDS],
    len: usize,
}

impl Default for RowsSnapshot {
    fn default() -> Self {
        Self {
            chunks: Vec::new(),
            index: std::array::from_fn(|_| None),
            len: 0,
        }
    }
}

impl PartialEq for EntryRows {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(left), Some(right)) => {
                Arc::ptr_eq(left, right)
                    || self.len() == other.len() && self.iter().eq(other.iter())
            }
            _ => false,
        }
    }
}

impl EntryRows {
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |snapshot| snapshot.len)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    pub fn get(&self, index: usize) -> Option<&Entry> {
        self.0
            .as_ref()?
            .chunks
            .get(index / ROWS_PER_CHUNK)?
            .get(index % ROWS_PER_CHUNK)
            .map(Arc::as_ref)
    }

    pub fn shared_iter(&self) -> impl Iterator<Item = &Arc<Entry>> {
        self.0
            .iter()
            .flat_map(|snapshot| snapshot.chunks.iter())
            .flat_map(|chunk| chunk.iter())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.shared_iter().map(Arc::as_ref)
    }

    pub fn retain(&mut self, mut select: impl FnMut(&Entry) -> bool) {
        *self = Self::from_shared(self.shared_iter().filter(|entry| select(entry)).cloned());
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0
            .as_ref()
            .and_then(|snapshot| snapshot.index[shard_for(key)].as_ref())
            .is_some_and(|index| index.contains_key(key))
    }

    pub fn merge(mut self, incoming: Self) -> Self {
        if self.is_empty() {
            return incoming;
        }
        self.extend_shared(incoming.shared_iter().cloned());
        self
    }

    fn from_shared(entries: impl IntoIterator<Item = Arc<Entry>>) -> Self {
        let mut result = Self::default();
        result.extend_shared(entries);
        result
    }

    fn extend_shared(&mut self, entries: impl IntoIterator<Item = Arc<Entry>>) {
        let mut entries = entries.into_iter().peekable();
        if entries.peek().is_none() {
            return;
        }
        let snapshot = Arc::make_mut(self.0.get_or_insert_with(Arc::default));
        for entry in entries {
            snapshot.insert(entry);
        }
    }
}

impl FromIterator<Entry> for EntryRows {
    fn from_iter<T: IntoIterator<Item = Entry>>(entries: T) -> Self {
        Self::from_shared(entries.into_iter().map(Arc::new))
    }
}

impl From<Vec<Entry>> for EntryRows {
    fn from(entries: Vec<Entry>) -> Self {
        entries.into_iter().collect()
    }
}

impl RowsSnapshot {
    fn insert(&mut self, entry: Arc<Entry>) {
        let shard = &mut self.index[shard_for(&entry.key)];
        if let Some(index) = shard
            .as_ref()
            .and_then(|index| index.get(entry.key.as_str()))
            .copied()
        {
            Arc::make_mut(&mut self.chunks[index / ROWS_PER_CHUNK])[index % ROWS_PER_CHUNK] = entry;
            return;
        }
        Arc::make_mut(shard.get_or_insert_with(Arc::default))
            .insert(Arc::from(entry.key.as_str()), self.len);
        if self.len.is_multiple_of(ROWS_PER_CHUNK) {
            self.chunks
                .push(Arc::new(Vec::with_capacity(ROWS_PER_CHUNK)));
        }
        Arc::make_mut(&mut self.chunks[self.len / ROWS_PER_CHUNK]).push(entry);
        self.len += 1;
    }
}

fn shard_for(key: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize % INDEX_SHARDS
}

#[derive(Clone, Default, PartialEq)]
pub struct Query {
    pub identity: Option<u64>,
    pub cursor: String,
    pub folder_id: Option<i64>,
    pub offset: i64,
    pub session: String,
    pub online: bool,
    pub previous: EntryRows,
}

#[derive(Clone, Default)]
pub struct Page {
    pub source: Option<PageSource>,
    pub items: EntryRows,
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
        let initial = Self {
            online: self.online,
            ..Default::default()
        };
        let query = if self.identity == Some(identity) {
            self
        } else {
            &initial
        };
        PageSource {
            identity,
            online: query.online,
            cursor: query.cursor.clone(),
            folder_id: query.folder_id,
            offset: query.offset,
            session: query.session.clone(),
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
        let source = query.source(identity);
        (self.has_more
            && self.source.as_ref() == Some(&source)
            && (self.offset > source.offset
                || (!self.cursor.is_empty() && self.cursor != source.cursor)))
            .then(|| Query {
                identity: Some(identity),
                cursor: self.cursor.clone(),
                folder_id: source.folder_id,
                offset: self.offset,
                session: self.session.clone(),
                online: source.online,
                previous: self.items.clone(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(index: usize, title: &str) -> Entry {
        Entry {
            key: format!("entry-{index}"),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn accumulated_pages_share_unchanged_rows_and_replace_overlap_in_place() {
        for total in [1_000, 5_000] {
            let size = 20;
            let mut current = EntryRows::default();
            for start in (0..total).step_by(size) {
                let incoming = (start..(start + size).min(total))
                    .map(|i| row(i, "original"))
                    .collect();
                let old = current.clone();
                current = current.merge(incoming);
                assert_eq!(old.len(), start);
                if start > 0 {
                    assert!(std::ptr::eq(old.get(0).unwrap(), current.get(0).unwrap()));
                }
            }
            let prior = current.clone();
            current = current.merge(
                vec![
                    row(129, "updated"),
                    row(total, "new"),
                    row(0, "also updated"),
                ]
                .into(),
            );
            assert_eq!(current.len(), total + 1);
            assert_eq!(current.get(129).unwrap().title, "updated");
            assert_eq!(current.get(0).unwrap().title, "also updated");
            assert_eq!(prior.get(129).unwrap().title, "original");
            assert!(std::ptr::eq(
                prior.get(128).unwrap(),
                current.get(128).unwrap()
            ));
            assert!(current.contains_key(&format!("entry-{total}")));
            assert!(!prior.contains_key(&format!("entry-{total}")));
            current.retain(|entry| entry.key != "entry-129");
            assert_eq!(current.len(), total);
            assert_eq!(current.get(129).unwrap().key, "entry-130");
            assert!(prior.contains_key("entry-129"));
            assert!(!current.contains_key("entry-129"));
            assert!(current.get(total).is_none());
        }
    }

    #[test]
    fn continuation_inherits_only_matching_scope_and_advancing_cursor() {
        let query = Query {
            identity: Some(12),
            online: true,
            folder_id: Some(33),
            offset: 20,
            session: "session".into(),
            ..Default::default()
        };
        let page = Page {
            source: Some(query.source(12)),
            has_more: true,
            offset: 40,
            session: "session".into(),
            items: vec![row(0, "a")].into(),
            ..Default::default()
        };
        let next = page.next_query(&query, 12).unwrap();
        assert_eq!(next.identity, Some(12));
        assert_eq!(next.folder_id, Some(33));
        assert_eq!(next.offset, 40);
        assert_eq!(next.session, "session");
        assert!(next.online);
        assert!(std::ptr::eq(
            next.previous.get(0).unwrap(),
            page.items.get(0).unwrap()
        ));
        assert!(page.next_query(&query, 13).is_none());
        assert!(
            page.next_query(
                &Query {
                    online: false,
                    ..query.clone()
                },
                12
            )
            .is_none()
        );
        assert!(
            page.next_query(
                &Query {
                    folder_id: Some(34),
                    ..query.clone()
                },
                12
            )
            .is_none()
        );
        assert!(Page { offset: 20, ..page }.next_query(&query, 12).is_none());
    }
}
