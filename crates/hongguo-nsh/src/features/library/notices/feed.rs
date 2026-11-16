use crate::bridge::Failure;
use hongguo_common::model::FollowNotice;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct Feed {
    pub(super) result: Resource<(u64, Vec<FollowNotice>), Failure>,
    pub(super) items: Memo<Vec<FollowNotice>>,
}

pub(super) fn load<F: Future<Output = Result<Vec<FollowNotice>, Failure>> + 'static>(
    logged_in: Memo<bool>,
    visible: Memo<bool>,
    identity: Memo<u64>,
    revision: Memo<u64>,
    fetch: impl Fn() -> F + 'static,
) -> Feed {
    let result = resource(
        move || (logged_in.get() && visible.get()).then(|| (identity.get(), revision.get())),
        move |(owner, _), _| {
            let request = fetch();
            async move { request.await.map(|items| (owner, items)) }
        },
    );
    let items = computed(move || {
        if !logged_in.get() {
            return Vec::new();
        }
        result
            .get()
            .filter(|(owner, _)| *owner == identity.get())
            .map(|(_, items)| items)
            .unwrap_or_default()
    });
    Feed { result, items }
}
