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

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    #[component]
    fn Host() -> impl IntoElement {
        let memory = CollectionMemory::new();
        let account = signal("A");
        view()
            .child(text(move || {
                format!(
                    "memory {} {}",
                    account.get(),
                    memory.overlay(account.get(), Vec::new()).len()
                )
            }))
            .child(
                crate::ui::button(
                    "Collect image in a comment",
                    crate::ui::ButtonVariant::Quiet,
                )
                .on_press(move || {
                    memory.remember(
                        "A",
                        Emoticon {
                            id: Some("echo".into()),
                            web_uri: "full-static".into(),
                            dynamic_url: Some("full-animated".into()),
                            ..Default::default()
                        },
                    )
                }),
            )
            .child(
                crate::ui::button("Switch account", crate::ui::ButtonVariant::Quiet).on_press(
                    move || {
                        account.set(if account.get_untracked() == "A" {
                            "B"
                        } else {
                            "A"
                        })
                    },
                ),
            )
            .child(
                crate::ui::button(
                    "Remove from another editor",
                    crate::ui::ButtonVariant::Quiet,
                )
                .on_press(move || memory.remove("A", "echo".into())),
            )
    }

    #[test]
    fn a_comment_collection_updates_other_editors_and_remains_account_scoped() -> Result<(), String>
    {
        let mut app = Headless::start(EngineConfig::new(Root::of::<Host>()))
            .map_err(|error| error.to_string())?;
        app.wait_for("memory A 0")?;
        app.press("Collect image in a comment")?;
        app.wait_for("memory A 1")?;
        app.press("Switch account")?;
        app.wait_for("memory B 0")?;
        app.press("Switch account")?;
        app.wait_for("memory A 1")?;
        app.press("Remove from another editor")?;
        app.wait_for("memory A 0")?;
        Ok(())
    }
    #[test]
    fn delayed_collection_index_keeps_the_complete_echo_and_cannot_resurrect_a_deletion() {
        let echo = Emoticon {
            id: Some("large-id".into()),
            web_uri: "static".into(),
            dynamic_url: Some("animated".into()),
            expand_web_url: Some("full".into()),
            image_type: 2,
            image_name: Some("a sticker".into()),
            ..Default::default()
        };
        let mut edits = OwnEdits::default();
        edits.remember(echo.clone());
        assert_eq!(edits.merge(vec![]), vec![echo.clone()]);
        assert_eq!(edits.merge(vec![echo.clone()]), vec![echo.clone()]);
        assert!(edits.added.is_empty());
        edits.remove("large-id".into());
        assert!(edits.merge(vec![echo]).is_empty());
    }
}
