use super::model::Feed;
use crate::i18n::tr;
use nsh_core::prelude::*;
use std::future::Future;

pub(super) fn like<F: Future<Output = Result<(), SharedString>> + 'static>(
    feed: Signal<Feed>,
    id: String,
    authenticated: bool,
    identity: Memo<Option<String>>,
    pending: Signal<bool>,
    error: Signal<SharedString>,
    request: impl FnOnce(String, bool) -> F + 'static,
) {
    if pending.get_untracked() {
        return;
    }
    if !authenticated {
        error.set(tr("player.interact.loginRequired"));
        return;
    }
    let mut updated = feed.get_untracked();
    let Some(before) = updated.toggle_like(&id) else {
        return;
    };
    feed.set(updated);
    pending.set(true);
    error.set(SharedString::default());
    let response = request(id.clone(), !before.0);
    let account = identity.get_untracked();
    spawn(async move {
        let result = response.await;
        pending.set(false);
        if account != identity.get_untracked() {
            return;
        }
        if let Err(failure) = result {
            feed.update(|feed| feed.restore_like(&id, before));
            error.set(failure);
        }
    });
}
