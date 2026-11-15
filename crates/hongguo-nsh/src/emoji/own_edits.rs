use super::remote_model::key;
use hongguo_api::emoticon::Emoticon;
use nsh_core::prelude::*;
use std::collections::{HashMap, HashSet};

/// Account-scoped collection echoes shared by every editor and comment image.
#[derive(Clone, Copy)]
pub(crate) struct CollectionMemory {
    values: Stored<HashMap<String, OwnEdits>>,
    revision: Signal<u64>,
}

impl CollectionMemory {
    pub(crate) fn new() -> Self {
        Self {
            values: stored(HashMap::new()),
            revision: signal(0),
        }
    }
    pub(super) fn remember(self, account: &str, item: Emoticon) {
        self.values
            .with(|values| values.entry(account.to_owned()).or_default().remember(item));
        self.revision.update(|revision| *revision += 1);
    }
    pub(super) fn remove(self, account: &str, key: String) {
        self.values
            .with(|values| values.entry(account.to_owned()).or_default().remove(key));
        self.revision.update(|revision| *revision += 1);
    }
    pub(super) fn merge(self, account: &str, page: Vec<Emoticon>) -> Vec<Emoticon> {
        self.values
            .with(|values| values.entry(account.to_owned()).or_default().merge(page))
    }
    pub(super) fn overlay(self, account: &str, items: Vec<Emoticon>) -> Vec<Emoticon> {
        self.revision.get();
        self.values
            .with(|values| values.entry(account.to_owned()).or_default().overlay(items))
    }
}

/// Optimistic collection index overlay. Keep full send echoes until the index acknowledges them.
#[derive(Clone, Default, PartialEq)]
pub(super) struct OwnEdits {
    added: Vec<Emoticon>,
    removed: HashSet<String>,
}

impl OwnEdits {
    pub fn remember(&mut self, item: Emoticon) {
        self.removed.remove(key(&item));
        self.added.retain(|old| key(old) != key(&item));
        self.added.insert(0, item);
    }
    pub fn remove(&mut self, identity: String) {
        self.added.retain(|item| key(item) != identity);
        self.removed.insert(identity);
    }
    pub fn merge(&mut self, items: Vec<Emoticon>) -> Vec<Emoticon> {
        self.added
            .retain(|added| !items.iter().any(|item| key(item) == key(added)));
        self.overlay(items)
    }
    fn overlay(&self, items: Vec<Emoticon>) -> Vec<Emoticon> {
        let mut result = self.added.clone();
        for item in items {
            if !self.removed.contains(key(&item))
                && !result.iter().any(|existing| key(existing) == key(&item))
            {
                result.push(item);
            }
        }
        result
    }
}
