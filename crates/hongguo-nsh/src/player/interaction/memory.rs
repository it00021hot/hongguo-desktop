use super::model::{Action, Overrides, Snapshot, Target, Toggle};
use nsh_core::prelude::*;

/// Session-owned successful interactions. No backend state or global singleton.
#[derive(Clone, Copy)]
pub(crate) struct SocialMemory {
    values: Stored<Overrides>,
    revision: Signal<u64>,
}

impl SocialMemory {
    pub(crate) fn new() -> Self {
        Self {
            values: stored(Overrides::default()),
            revision: signal(0),
        }
    }
    pub(super) fn refresh(
        self,
        account: &str,
        target: &Target,
        server: Snapshot,
        current: Snapshot,
    ) -> Snapshot {
        self.revision.get();
        let mut next = self
            .values
            .with(|values| values.apply(account, target, server));
        // Another screen's completion must not clear this screen's in-flight mutation.
        if current.like.pending {
            next.like = current.like;
        }
        if current.collect.pending {
            next.collect = current.collect;
        }
        if current.follow.pending {
            next.follow = current.follow;
        }
        next
    }
    pub(super) fn remember(self, account: &str, target: &Target, action: Action, value: Toggle) {
        self.values
            .with(|values| values.remember(account, target, action, value));
        self.revision.update(|revision| *revision += 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    #[component]
    fn Host() -> impl IntoElement {
        let memory = SocialMemory::new();
        let detail_memory = memory;
        let account = signal("A");
        let target = stored(Target {
            series_id: "series".into(),
            vid: "episode-1".into(),
            title: String::new(),
            likes: 0,
            collections: 0,
            comments: 0,
        });
        let detail = signal(Snapshot::default());
        effect(move || {
            detail.set(detail_memory.refresh(
                account.get(),
                &target.get(),
                Snapshot::default(),
                detail.get_untracked(),
            ))
        });
        view()
            .child(text(move || {
                format!(
                    "detail:{}:{}:{}",
                    account.get(),
                    detail.get().collect.selected,
                    detail.get().like.pending
                )
            }))
            .child(
                ui::button("Player collect success", ui::ButtonVariant::Quiet).on_press(
                    move || {
                        memory.remember("A", &target.get(), Action::Collect, Toggle::new(true, 1))
                    },
                ),
            )
            .child(
                ui::button("Detail pending like", ui::ButtonVariant::Quiet).on_press(move || {
                    detail.update(|detail| {
                        detail.begin(Action::Like);
                    })
                }),
            )
            .child(
                ui::button("Player follow success", ui::ButtonVariant::Quiet).on_press(move || {
                    memory.remember("A", &target.get(), Action::Follow, Toggle::new(true, 1))
                }),
            )
            .child(
                ui::button("Switch account", ui::ButtonVariant::Quiet).on_press(move || {
                    detail.set(Snapshot::default());
                    account.set("B");
                }),
            )
    }
    #[test]
    fn a_second_screen_receives_successful_interactions_without_erasing_pending_work_or_crossing_accounts()
    -> Result<(), String> {
        let mut app = Headless::start(EngineConfig::new(Root::of::<Host>()))
            .map_err(|error| error.to_string())?;
        app.wait_for("text:detail:A:false:false")?;
        app.press("Player collect success")?;
        app.wait_for("text:detail:A:true:false")?;
        app.press("Detail pending like")?;
        app.press("Player follow success")?;
        app.wait_for("text:detail:A:true:true")?;
        app.press("Switch account")?;
        app.wait_for("text:detail:B:false:false")?;
        Ok(())
    }
}
