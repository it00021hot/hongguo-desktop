use super::SessionHandle;
use crate::navigation::Navigation;
use nsh_core::prelude::*;

mod account;
mod preferences;

#[derive(Clone, Copy)]
struct Tasks {
    owner: Owner,
    pending: Signal<bool>,
}

impl Tasks {
    fn provide() -> Self {
        if let Some(tasks) = use_context::<Self>() {
            return tasks;
        }
        let tasks = Self {
            owner: Owner::current(),
            pending: signal(false),
        };
        provide_context(tasks);
        tasks
    }
}

pub(crate) use account::{dialogs as account_dialogs, preview_dialogs as preview_account_dialogs};

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    preferences::page(session, nav)
}

pub(super) fn preview(nav: Navigation) -> Screen {
    preferences::preview(nav)
}

pub(super) fn account(session: SessionHandle, nav: Navigation) -> Screen {
    account::fallback(session, nav)
}
