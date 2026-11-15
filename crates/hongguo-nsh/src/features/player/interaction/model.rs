pub(super) use hongguo_service::interaction_service::model::{Action, Snapshot, Target, Toggle};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Overrides(HashMap<(String, Action, String), Toggle>);

impl Overrides {
    fn key(account: &str, target: &Target, action: Action) -> (String, Action, String) {
        (
            account.to_owned(),
            action,
            if action == Action::Like {
                target.vid.clone()
            } else {
                target.series_id.clone()
            },
        )
    }

    pub(super) fn apply(&self, account: &str, target: &Target, mut snapshot: Snapshot) -> Snapshot {
        for action in [Action::Like, Action::Collect, Action::Follow] {
            if let Some(value) = self.0.get(&Self::key(account, target, action)) {
                *snapshot.value_mut(action) = *value;
            }
        }
        snapshot
    }

    pub(super) fn remember(
        &mut self,
        account: &str,
        target: &Target,
        action: Action,
        value: Toggle,
    ) {
        self.0.insert(Self::key(account, target, action), value);
    }
}
