use super::model::{Feed, Item, Page};
use crate::bridge::Failure;
use nsh_core::prelude::*;
use std::{collections::HashMap, future::Future, pin::Pin, rc::Rc};

type Load = Rc<dyn Fn(String, String) -> Pin<Box<dyn Future<Output = Result<Page, Failure>>>>>;

#[derive(Clone, Copy)]
pub(super) struct ReplyThread {
    pub expanded: Signal<bool>,
    pub editing: Signal<bool>,
    pub feed: Signal<Feed>,
    pub cursor: Signal<String>,
    pub recipient: Signal<Option<(String, String)>>,
    pub message: Signal<SharedString>,
    pub loaded: Resource<(String, Option<String>, Page), Failure>,
}

#[derive(Clone, Copy)]
pub(super) struct Threads {
    rows: Stored<HashMap<String, ReplyThread>>,
    revision: Signal<u64>,
    owner: Stored<nsh_core::element::Owner>,
    actions: Stored<HashMap<(Option<String>, String), ActionState>>,
    pub account: Memo<Option<String>>,
}

#[derive(Clone, Copy)]
pub(super) struct ActionState {
    pub pending: Signal<bool>,
    pub error: Signal<SharedString>,
}

impl Threads {
    pub fn new<F: Future<Output = Result<Page, Failure>> + 'static>(
        roots: Signal<Feed>,
        account: Memo<Option<String>>,
        load: impl Fn(String, String) -> F + 'static,
    ) -> Self {
        let state = Self {
            rows: stored(HashMap::new()),
            revision: signal(0),
            owner: stored(nsh_core::element::Owner::current()),
            actions: stored(HashMap::new()),
            account,
        };
        let load: Load = Rc::new(move |parent, cursor| Box::pin(load(parent, cursor)));
        let owner = nsh_core::element::Owner::current();
        effect(move || {
            let items = roots.get().items;
            let load = load.clone();
            owner.run(|| {
                for item in items {
                    let id = item.id().to_owned();
                    if state.rows.with(|rows| rows.contains_key(&id)) {
                        continue;
                    }
                    let thread = ReplyThread::new(id.clone(), account, load.clone());
                    state.rows.with(|rows| {
                        rows.insert(id, thread);
                    });
                    state.revision.update(|value| *value += 1);
                }
            });
        });
        state
    }

    pub fn get(self, parent: &str) -> Option<ReplyThread> {
        self.revision.get();
        self.rows.with(|rows| rows.get(parent).copied())
    }

    pub fn action(self, id: &str) -> ActionState {
        let key = (self.account.get_untracked(), id.to_owned());
        if let Some(state) = self.actions.with(|actions| actions.get(&key).copied()) {
            return state;
        }
        let state = self.run(|| ActionState {
            pending: signal(false),
            error: signal(SharedString::default()),
        });
        self.actions.with(|actions| {
            actions.insert(key, state);
        });
        state
    }

    pub fn run<R>(self, action: impl FnOnce() -> R) -> R {
        let owner = self.owner.get();
        owner.run(action)
    }
}

impl ReplyThread {
    fn new(parent: String, account: Memo<Option<String>>, load: Load) -> Self {
        let expanded = signal(false);
        let editing = signal(false);
        let feed = signal(Feed::default());
        let cursor = signal(String::new());
        let recipient = signal(None);
        let message = signal(SharedString::default());
        let previous = stored(account.get_untracked());
        effect(move || {
            let current = account.get();
            if previous.replace(current.clone()) != current {
                feed.set(Feed::default());
                cursor.set(String::new());
                recipient.set(None);
                expanded.set(false);
                editing.set(false);
                message.set(SharedString::default());
            }
        });
        let loaded = resource(
            move || expanded.get().then(|| (cursor.get(), account.get())),
            move |(cursor, account), _| {
                let future = load(parent.clone(), cursor.clone());
                async move { future.await.map(|page| (cursor, account, page)) }
            },
        );
        effect(move || {
            if !loaded.loading()
                && loaded.error().is_none()
                && let Some((cursor, identity, page)) = loaded.get()
                && identity == account.get_untracked()
            {
                feed.update(|feed| feed.append(page, &cursor));
            }
        });
        Self {
            expanded,
            editing,
            feed,
            cursor,
            recipient,
            message,
            loaded,
        }
    }
}

#[derive(Clone, PartialEq)]
pub(super) enum Entry {
    Root(Item),
    Reply { parent: String, item: Item },
    Footer(String),
}

impl Entry {
    pub fn key(&self) -> String {
        match self {
            Self::Root(item) => format!("root:{}", item.id()),
            Self::Reply { parent, item } => format!("reply:{parent}:{}", item.id()),
            Self::Footer(parent) => format!("footer:{parent}"),
        }
    }
}

pub(super) fn entries(feed: Signal<Feed>, threads: Threads) -> Memo<Vec<Entry>> {
    computed(move || {
        let mut entries = Vec::new();
        for item in feed.get().items {
            let parent = item.id().to_owned();
            entries.push(Entry::Root(item));
            if let Some(thread) = threads.get(&parent)
                && thread.expanded.get()
            {
                entries.extend(
                    thread
                        .feed
                        .get()
                        .items
                        .into_iter()
                        .map(|item| Entry::Reply {
                            parent: parent.clone(),
                            item,
                        }),
                );
                entries.push(Entry::Footer(parent));
            }
        }
        entries
    })
}
