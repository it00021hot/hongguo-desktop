use super::model::{Entry, Page, Query};
use crate::bridge::Failure;
use nsh_core::prelude::*;

pub(super) type Feed = Resource<Page, Failure>;

#[derive(Clone, Copy)]
pub(super) struct Context {
    pub(super) logged_in: Memo<bool>,
    pub(super) active: Memo<bool>,
    pub(super) identity: Memo<u64>,
    pub(super) revision: Memo<u64>,
    pub(super) search: Option<Signal<String>>,
}

pub(super) fn load<F: Future<Output = Result<Page, Failure>> + 'static>(
    query: Signal<Query>,
    context: Context,
    fetch: impl Fn(Query) -> F + 'static,
) -> Feed {
    let feed = resource(
        move || {
            (context.logged_in.get() && context.active.get()).then(|| {
                let identity = context.identity.get();
                (
                    query.get().for_identity(identity),
                    identity,
                    context.revision.get(),
                )
            })
        },
        move |(query, identity, _), _| {
            let source = query.source(identity);
            let request = fetch(query);
            async move {
                request.await.map(|mut page| {
                    page.source = Some(source);
                    page
                })
            }
        },
    );
    if let Some(search) = context.search {
        effect(move || {
            if !search.with(String::is_empty) {
                advance(feed, query, context);
            }
        });
    }
    feed
}

pub(super) fn current_items(
    feed: Feed,
    query: Signal<Query>,
    context: Context,
) -> Memo<Vec<Entry>> {
    computed(move || {
        if !context.logged_in.get() {
            return Vec::new();
        }
        feed.with(|page| {
            page.filter(|page| page.belongs_to(&query.get(), context.identity.get()))
                .map(|page| page.items.clone())
                .unwrap_or_default()
        })
    })
}

pub(super) fn advance(feed: Feed, query: Signal<Query>, context: Context) {
    if !context.logged_in.get_untracked()
        || !context.active.get_untracked()
        || feed.loading()
        || feed.error().is_some()
    {
        return;
    }
    if let Some(next) = feed.with(|page| {
        page.and_then(|page| {
            page.next_query(&query.get_untracked(), context.identity.get_untracked())
        })
    }) {
        query.set(next);
    }
}

#[cfg(test)]
mod tests;
