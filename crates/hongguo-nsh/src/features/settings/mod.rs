pub(crate) mod presentation;
use super::SessionHandle;
use crate::navigation::Navigation;
use nsh_core::prelude::*;

mod account;
mod preferences;

pub(crate) fn provide_tasks() {
    Tasks::provide();
}

#[derive(Clone, Copy)]
struct Tasks {
    owner: Owner,
    updates: Stored<std::sync::Arc<tokio::sync::Mutex<()>>>,
}

impl Tasks {
    fn provide() -> Self {
        if let Some(tasks) = use_context::<Self>() {
            return tasks;
        }
        let tasks = Self {
            owner: Owner::current(),
            updates: stored(std::sync::Arc::new(tokio::sync::Mutex::new(()))),
        };
        provide_context(tasks);
        tasks
    }
}

pub(crate) use account::dialogs as account_dialogs;
pub(crate) use account::profile;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    preferences::page(session, nav)
}

pub(super) fn account(session: SessionHandle, nav: Navigation) -> Screen {
    account::fallback(session, nav)
}
