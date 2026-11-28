use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use hongguo_service::library_service::{
    self, CollectionMetadata, METADATA_LOCAL_BATCH, METADATA_WINDOW_BATCH,
};
use nsh_core::{element::ViewableItems, prelude::*};

use super::{feed::Context, model::Entry};
use crate::{bridge, events::SessionHandle};

const FIRST_WINDOW: usize = 12;
const MAX_CONSECUTIVE_SEARCH_FAILURES: u8 = 3;
pub(super) const VIEW_SETTLE_TIME: std::time::Duration = std::time::Duration::from_millis(120);

#[derive(Clone, Default, PartialEq)]
struct Cache {
    cards: HashMap<String, CollectionMetadata>,
    titles: HashMap<String, (String, String)>,
}

#[derive(Clone, Copy)]
pub(super) struct Cards {
    identity: Memo<u64>,
    cache: Signal<(u64, Cache)>,
    range: Signal<Option<(usize, usize)>>,
    search_revision: Signal<u64>,
    progress: Signal<Option<Progress>>,
}

#[derive(Clone)]
struct Scope(Rc<Vec<String>>);

impl PartialEq for Scope {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, PartialEq)]
struct Progress {
    identity: u64,
    scope: Scope,
    local: usize,
    remote: usize,
    incomplete: bool,
    consecutive_failures: u8,
}

impl Progress {
    fn for_scope(previous: Option<Self>, scope: Scope, identity: u64) -> Self {
        if let Some(mut previous) = previous
            && previous.identity == identity
            && (previous.scope == scope || scope.0.starts_with(previous.scope.0.as_slice()))
        {
            previous.scope = scope;
            return previous;
        }
        Self {
            identity,
            scope,
            local: 0,
            remote: 0,
            incomplete: false,
            consecutive_failures: 0,
        }
    }
}

#[derive(Clone, PartialEq)]
enum Work {
    Viewport,
    Local(Progress),
    Remote(Progress),
}

#[derive(Clone, PartialEq)]
struct Request {
    identity: u64,
    ids: Vec<String>,
    work: Work,
}

impl Cards {
    pub(super) fn new(identity: Memo<u64>) -> Self {
        Self {
            identity,
            cache: signal((identity.get_untracked(), Cache::default())),
            range: signal(None),
            search_revision: signal(0),
            progress: signal(None),
        }
    }

    pub(super) fn report(self, items: &ViewableItems) {
        if self.range.get_untracked() != items.viewable {
            self.range.set(items.viewable);
        }
    }

    pub(super) fn reset(self) {
        self.range.set(None);
    }

    pub(super) fn retry_search(self) {
        batch(|| {
            self.cache.update(|(_, cache)| {
                cache
                    .cards
                    .retain(|_, item| matches!(item, CollectionMetadata::Ready { .. }));
            });
            self.progress.update(|progress| {
                if let Some(progress) = progress {
                    progress.local = 0;
                    progress.remote = 0;
                    progress.incomplete = false;
                    progress.consecutive_failures = 0;
                }
            });
        });
    }

    pub(super) fn status(self, entry: &Entry) -> Option<CollectionMetadata> {
        let identity = self.identity.get();
        self.cache.with(|(owner, cache)| {
            (*owner == identity)
                .then(|| cache.cards.get(&entry.series_id).cloned())
                .flatten()
        })
    }

    pub(super) fn hydrate(self, entry: &mut Entry) {
        let identity = self.identity.get();
        self.cache.with(|(owner, cache)| {
            if *owner != identity {
                return;
            }
            if let Some(metadata) = cache.cards.get(&entry.series_id) {
                metadata.apply(entry);
            }
            if entry.title.trim().is_empty()
                && let Some((title, _)) = cache.titles.get(&entry.series_id)
            {
                entry.title.clone_from(title);
            }
        });
    }

    pub(super) fn filter<E: Borrow<Entry> + Clone>(self, entries: &[E], text: &str) -> Vec<E> {
        if text.is_empty() {
            return entries.to_vec();
        }
        let identity = self.identity.get();
        self.search_revision.get();
        self.cache.with_untracked(|(owner, cache)| {
            entries
                .iter()
                .filter(|entry| {
                    let entry = (*entry).borrow();
                    entry.title.to_lowercase().contains(text)
                        || entry.series_id.to_lowercase().contains(text)
                        || entry.liked_vid.as_ref().is_some_and(|id| id.contains(text))
                        || entry
                            .collection_item_id
                            .as_ref()
                            .is_some_and(|id| id.to_lowercase().contains(text))
                        || (*owner == identity
                            && cache
                                .titles
                                .get(&entry.series_id)
                                .is_some_and(|(_, folded)| folded.contains(text)))
                })
                .cloned()
                .collect()
        })
    }

    pub(super) fn searching<E: 'static>(self, all: Memo<Vec<E>>, search: Signal<String>) -> bool {
        !search.with(|value| value.trim().is_empty())
            && self.progress.with(|progress| match progress {
                Some(progress) if progress.identity == self.identity.get() => {
                    progress.remote < progress.scope.0.len()
                        && progress.consecutive_failures < MAX_CONSECUTIVE_SEARCH_FAILURES
                }
                _ => all.with(|entries| !entries.is_empty()),
            })
    }

    pub(super) fn search_incomplete<E: 'static>(self, _all: Memo<Vec<E>>) -> bool {
        self.progress.with(|progress| {
            progress.as_ref().is_some_and(|progress| {
                progress.identity == self.identity.get() && progress.incomplete
            })
        })
    }

    pub(super) fn drive<E: Borrow<Entry> + 'static>(
        self,
        session: SessionHandle,
        context: Context,
        active: impl Fn() -> bool + Copy + 'static,
        visible: Memo<Vec<E>>,
        searchable: Memo<Vec<E>>,
        search: Signal<String>,
    ) {
        let search_active = computed(move || search.with(|value| !value.trim().is_empty()));
        let scope = computed(move || {
            search_active.get().then(|| {
                Scope(Rc::new(searchable.with(|entries| {
                    let mut seen = HashSet::new();
                    entries
                        .iter()
                        .filter(|entry| {
                            let entry = (*entry).borrow();
                            entry.title.trim().is_empty()
                                && !entry.series_id.is_empty()
                                && seen.insert(entry.series_id.as_str())
                        })
                        .map(|entry| entry.borrow().series_id.clone())
                        .collect()
                })))
            })
        });
        effect(move || {
            let Some(scope) = scope.get() else {
                return;
            };
            let identity = self.identity.get();
            let previous = self.progress.get_untracked();
            if !previous
                .as_ref()
                .is_some_and(|current| current.identity == identity && current.scope == scope)
            {
                self.progress
                    .set(Some(Progress::for_scope(previous, scope, identity)));
            }
        });
        let plan = computed(move || {
            if !context.logged_in.get() || !context.active.get() || !active() {
                return None;
            }
            let identity = self.identity.get();
            self.cache.with(|(owner, cache)| {
                let own = *owner == identity;
                if let Some(scope) = scope.get() {
                    let progress = Progress::for_scope(self.progress.get(), scope, identity);
                    if let Some((ids, work)) = search_batch(progress, own.then_some(cache)) {
                        return Some(Request {
                            identity,
                            ids,
                            work,
                        });
                    }
                }
                let range = self.range.get();
                let ids = visible.with(|visible| {
                    next_ids(visible, range, |id| !own || !cache.cards.contains_key(id))
                });
                (!ids.is_empty()).then_some(Request {
                    identity,
                    ids,
                    work: Work::Viewport,
                })
            })
        });
        let loaded = resource(
            move || plan.get(),
            move |request, _| {
                let state = session.with(|session| session.state.clone());
                async move {
                    let ids = request.ids.clone();
                    let result = if ids.is_empty() {
                        Ok(HashMap::new())
                    } else {
                        match &request.work {
                            Work::Local(_) => {
                                bridge::request(library_service::cached_collection_metadata(
                                    state, ids,
                                ))
                                .await
                            }
                            _ => {
                                bridge::request(library_service::collection_metadata(state, ids))
                                    .await
                            }
                        }
                    };
                    Ok::<_, std::convert::Infallible>((request, result))
                }
            },
        );
        effect(move || {
            let Some((request, response)) = loaded.get() else {
                return;
            };
            if plan.get_untracked().as_ref() != Some(&request) {
                return;
            }
            let filter = search.with_untracked(|value| value.trim().to_lowercase());
            let mut changed_results = false;
            let mut incomplete = false;
            let mut all_failed = !request.ids.is_empty();
            batch(|| {
                self.cache.update(|(owner, cache)| {
                if *owner != request.identity {
                    *owner = request.identity;
                    *cache = Cache::default();
                }
                let mut result = response.unwrap_or_default();
                for id in &request.ids {
                    let item = result.remove(id).unwrap_or(CollectionMetadata::Failed);
                    all_failed &= item == CollectionMetadata::Failed;
                    let title = match &item {
                        CollectionMetadata::Ready {title, ..} => Some(title.clone()),
                        _ => None,
                    };
                    if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                        let previous_match = cache.titles.get(id).is_some_and(|(_, folded)| folded.contains(&filter));
                        changed_results |= !filter.is_empty() && previous_match != title.to_lowercase().contains(&filter);
                        cache.titles.insert(id.clone(), (title.clone(), title.to_lowercase()));
                    } else if matches!(request.work, Work::Remote(_)) && !cache.titles.contains_key(id) {
                        incomplete = true;
                    }
                    let full_local = matches!(&item, CollectionMetadata::Ready {cover, episode_count, ..}
                        if !cover.is_empty() && *episode_count > 0);
                    if !matches!(request.work, Work::Local(_)) || full_local {
                        cache.cards.insert(id.clone(), item);
                    }
                }
            });
                if changed_results {
                    self.search_revision
                        .update(|revision| *revision = revision.wrapping_add(1));
                }
                let remote = matches!(&request.work, Work::Remote(_));
                if let Work::Local(mut progress) | Work::Remote(mut progress) = request.work {
                    if remote && !request.ids.is_empty() {
                        progress.consecutive_failures = if all_failed {
                            progress.consecutive_failures.saturating_add(1)
                        } else {
                            0
                        };
                    }
                    progress.incomplete |= incomplete;
                    self.progress.set(Some(progress));
                }
            });
        });
    }
}

fn search_batch(mut progress: Progress, cache: Option<&Cache>) -> Option<(Vec<String>, Work)> {
    let ids = &progress.scope.0;
    if progress.local < ids.len() {
        let end = (progress.local + METADATA_LOCAL_BATCH).min(ids.len());
        let batch = ids[progress.local..end]
            .iter()
            .filter(|id| !cache.is_some_and(|cache| cache.titles.contains_key(*id)))
            .cloned()
            .collect();
        progress.local = end;
        return Some((batch, Work::Local(progress)));
    }
    if progress.remote == ids.len()
        || progress.consecutive_failures >= MAX_CONSECUTIVE_SEARCH_FAILURES
    {
        return None;
    }
    let mut batch = Vec::new();
    while progress.remote < ids.len() && batch.len() < METADATA_WINDOW_BATCH {
        let id = &ids[progress.remote];
        progress.remote += 1;
        if cache.is_some_and(|cache| cache.titles.contains_key(id)) {
            continue;
        }
        if cache.is_some_and(|cache| cache.cards.contains_key(id)) {
            progress.incomplete = true;
            continue;
        }
        batch.push(id.clone());
    }
    Some((batch, Work::Remote(progress)))
}

fn next_ids<E: Borrow<Entry>>(
    visible: &[E],
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
    [on_screen, after, before]
        .into_iter()
        .flatten()
        .map(|entry| entry.borrow().series_id.as_str())
        .filter(|id| !id.is_empty() && missing(id) && seen.insert(*id))
        .take(METADATA_WINDOW_BATCH)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_thousand_titles_use_bounded_indexed_reads_and_reach_the_last_row() {
        let ids = (0..5000).map(|i| i.to_string()).collect::<Vec<_>>();
        let mut cache = Cache::default();
        let mut progress = Progress {
            identity: 1,
            scope: Scope(Rc::new(ids)),
            local: 0,
            remote: 0,
            incomplete: false,
            consecutive_failures: 0,
        };
        let mut local_batches = 0;
        let mut remote_ids = Vec::new();
        while let Some((ids, work)) = search_batch(progress.clone(), Some(&cache)) {
            match work {
                Work::Local(next) => {
                    assert!(ids.len() <= METADATA_LOCAL_BATCH);
                    local_batches += 1;
                    for id in ids.into_iter().filter(|id| id != "4999") {
                        cache
                            .titles
                            .insert(id, ("Local Title".into(), "local title".into()));
                    }
                    progress = next;
                }
                Work::Remote(next) => {
                    remote_ids.extend(ids);
                    progress = next;
                }
                Work::Viewport => unreachable!(),
            }
        }
        assert_eq!(local_batches, 20);
        assert_eq!(remote_ids, ["4999"]);
        assert_eq!(progress.remote, 5000);
    }

    #[test]
    fn appending_a_page_keeps_cursors_but_new_account_or_filter_resets() {
        let mut first = Progress::for_scope(None, Scope(Rc::new(vec!["a".into(), "b".into()])), 10);
        first.local = 2;
        first.remote = 2;
        let extended = Scope(Rc::new(vec!["a".into(), "b".into(), "c".into()]));
        let next = Progress::for_scope(Some(first.clone()), extended.clone(), 10);
        assert_eq!((next.local, next.remote), (2, 2));
        let new_account = Progress::for_scope(Some(first), extended, 11);
        assert_eq!((new_account.local, new_account.remote), (0, 0));
        let changed = Progress::for_scope(Some(next), Scope(Rc::new(vec!["b".into()])), 10);
        assert_eq!((changed.local, changed.remote), (0, 0));
    }

    #[test]
    fn repeated_transport_failures_stop_the_background_title_scan() {
        let progress = Progress {
            identity: 1,
            scope: Scope(Rc::new(vec!["missing".into()])),
            local: 1,
            remote: 0,
            incomplete: true,
            consecutive_failures: MAX_CONSECUTIVE_SEARCH_FAILURES,
        };
        assert!(search_batch(progress, None).is_none());
    }

    #[test]
    fn unknown_title_behind_a_failed_viewport_load_marks_search_incomplete() {
        let mut cache = Cache::default();
        cache
            .cards
            .insert("missing".into(), CollectionMetadata::Failed);
        let progress = Progress {
            identity: 1,
            scope: Scope(Rc::new(vec!["missing".into()])),
            local: 1,
            remote: 0,
            incomplete: false,
            consecutive_failures: 0,
        };
        let Some((ids, Work::Remote(next))) = search_batch(progress, Some(&cache)) else {
            panic!("the scan must advance to completion");
        };
        assert!(ids.is_empty());
        assert!(next.incomplete);
        assert_eq!(next.remote, 1);
    }
}
