use std::collections::{HashMap, HashSet};

use hongguo_service::library_service::{self, CollectionMetadata, METADATA_WINDOW_BATCH};
use nsh_core::{element::ViewableItems, prelude::*};

use super::{feed::Context, model::Entry};
use crate::{bridge, events::SessionHandle};

const FIRST_WINDOW: usize = 12;
pub(super) const VIEW_SETTLE_TIME: std::time::Duration = std::time::Duration::from_millis(120);

#[derive(Clone, Copy)]
pub(super) struct Cards {
    identity: Memo<u64>,
    cache: Signal<(u64, HashMap<String, CollectionMetadata>)>,
    range: Signal<Option<(usize, usize)>>,
}

#[derive(Clone, PartialEq)]
struct Request {
    identity: u64,
    ids: Vec<String>,
}

impl Cards {
    pub(super) fn new(identity: Memo<u64>) -> Self {
        Self {
            identity,
            cache: signal((identity.get_untracked(), HashMap::new())),
            range: signal(None),
        }
    }

    pub(super) fn report(self, items: &ViewableItems) {
        self.range.set(items.viewable);
    }

    pub(super) fn reset(self) {
        self.range.set(None);
        // Retry transient failures when a new collection view is entered.
        self.cache
            .update(|(_, entries)| entries.retain(|_, value| *value != CollectionMetadata::Failed));
    }

    pub(super) fn status(self, entry: &Entry) -> Option<CollectionMetadata> {
        let identity = self.identity.get();
        self.cache.with(|(owner, entries)| {
            (*owner == identity)
                .then(|| entries.get(&entry.series_id).cloned())
                .flatten()
        })
    }

    pub(super) fn hydrate(self, entry: &mut Entry) {
        if let Some(metadata) = self.status(entry) {
            metadata.apply(entry);
        }
    }

    pub(super) fn matches(self, entry: &Entry, text: &str) -> bool {
        entry.title.to_lowercase().contains(text)
            || entry.series_id.to_lowercase().contains(text)
            || entry
                .collection_item_id
                .as_ref()
                .is_some_and(|id| id.to_lowercase().contains(text))
            || matches!(self.status(entry), Some(CollectionMetadata::Ready { title, .. }) if title.to_lowercase().contains(text))
    }

    pub(super) fn searching(self, all: Memo<Vec<Entry>>, search: Signal<String>) -> bool {
        !search.with(|value| value.trim().is_empty())
            && all.with(|entries| entries.iter().any(|entry| self.status(entry).is_none()))
    }

    pub(super) fn search_incomplete(self, all: Memo<Vec<Entry>>) -> bool {
        all.with(|entries| {
            entries
                .iter()
                .any(|entry| self.status(entry) == Some(CollectionMetadata::Failed))
        })
    }

    pub(super) fn drive(
        self,
        session: SessionHandle,
        context: Context,
        active: impl Fn() -> bool + Copy + 'static,
        visible: Memo<Vec<Entry>>,
        searchable: Memo<Vec<Entry>>,
        search: Signal<String>,
    ) {
        let plan = computed(move || {
            if !context.logged_in.get() || !context.active.get() || !active() {
                return None;
            }
            let identity = self.identity.get();
            let range = self.range.get();
            let searching = !search.with(|value| value.trim().is_empty());
            let ids = self.cache.with(|(owner, entries)| {
                let missing = |id: &str| *owner != identity || !entries.contains_key(id);
                visible.with(|visible| {
                    if searching {
                        searchable.with(|all| next_ids(visible, all, range, missing))
                    } else {
                        next_ids(visible, &[], range, missing)
                    }
                })
            });
            (!ids.is_empty()).then_some(Request { identity, ids })
        });
        let loaded = resource(
            move || plan.get(),
            move |request, _| {
                let state = session.with(|session| session.state.clone());
                async move {
                    let result = bridge::request(library_service::collection_metadata(
                        state,
                        request.ids.clone(),
                    ))
                    .await;
                    Ok::<_, std::convert::Infallible>((request, result))
                }
            },
        );
        effect(move || {
            let Some((request, result)) = loaded.get() else {
                return;
            };
            if plan.get_untracked().as_ref() != Some(&request) {
                return;
            }
            self.cache.update(|(owner, entries)| {
                if *owner != request.identity {
                    *owner = request.identity;
                    entries.clear();
                }
                match result {
                    Ok(result) => entries.extend(result),
                    Err(_) => entries.extend(
                        request
                            .ids
                            .into_iter()
                            .map(|id| (id, CollectionMetadata::Failed)),
                    ),
                }
            });
        });
    }
}

fn next_ids(
    visible: &[Entry],
    search: &[Entry],
    range: Option<(usize, usize)>,
    missing: impl Fn(&str) -> bool,
) -> Vec<String> {
    let count = visible.len();
    let [on_screen, after, before] = if let Some((lo, hi)) = range {
        let start = lo.min(count);
        let end = hi.saturating_add(1).clamp(start, count);
        let overscan = end - start;
        [
            &visible[start..end],
            &visible[end..end.saturating_add(overscan).min(count)],
            &visible[start.saturating_sub(overscan)..start],
        ]
    } else {
        [&visible[..FIRST_WINDOW.min(count)], &[], &[]]
    };
    let mut seen = HashSet::new();
    [on_screen, after, before, search]
        .into_iter()
        .flatten()
        .map(|entry| entry.series_id.as_str())
        .filter(|id| missing(id) && seen.insert(*id))
        .take(METADATA_WINDOW_BATCH)
        .map(str::to_owned)
        .collect()
}
