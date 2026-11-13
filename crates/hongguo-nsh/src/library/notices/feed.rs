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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui;
    use nsh_engine::{Clock, EngineConfig, Root};
    use nsh_testing::Headless;
    use std::time::{Duration, SystemTime};

    #[component]
    fn AccountBoundary() -> impl IntoElement {
        let logged_in = signal(false);
        let identity = signal(0);
        let revision = signal(0);
        let reads = signal(0);
        let active = signal(false);
        let feed = load(
            computed(move || logged_in.get()),
            computed(|| true),
            computed(move || identity.get()),
            computed(move || revision.get()),
            move || {
                reads.update(|reads| *reads += 1);
                let owner = identity.get_untracked();
                async move {
                    timer(Duration::from_millis(250)).await;
                    let mut items = crate::library::fixtures::notice_samples();
                    items[0].title = format!("account{owner}");
                    Ok(items)
                }
            },
        );
        super::super::frame(
            active,
            computed(move || super::super::unread(&feed.items.get())),
            computed(move || logged_in.get()),
            move || logged_in.set(true),
        )
        .child(text(move || format!("reads:{}", reads.get())))
        .child(text(move || format!("items:{}", feed.items.with(Vec::len))))
        .child(text(move || {
            feed.items
                .get()
                .first()
                .map(|notice| notice.title.clone())
                .unwrap_or_default()
        }))
        .child(
            ui::button("Log out", ui::ButtonVariant::Quiet).on_press(move || {
                batch(|| {
                    logged_in.set(false);
                    identity.update(|identity| *identity += 1);
                })
            }),
        )
        .child(
            ui::button("Refresh backend", ui::ButtonVariant::Quiet)
                .on_press(move || revision.update(|revision| *revision += 1)),
        )
    }

    #[test]
    fn anonymous_tabs_never_read_storage_and_logout_masks_retained_account_data()
    -> Result<(), String> {
        let mut config = EngineConfig::new(
            Root::of::<AccountBoundary>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
        );
        config.clock = Clock::manual(SystemTime::UNIX_EPOCH);
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("text:reads:0")?;
        app.wait_for("text:items:0")?;
        app.wait_for("Sign in to follow series")?;
        assert!(app.screen().enabled("role:button[Updates]")?);
        app.press("role:button[Updates]")?;
        app.wait_for("Sign in to follow series")?;
        app.wait_for("text:reads:0")?;
        app.press("role:button[Sign in]")?;
        app.wait_for("account0")?;
        app.wait_for("text:reads:1")?;
        app.press("Log out")?;
        app.wait_for("Sign in to follow series")?;
        app.wait_for("text:items:0")?;
        assert!(!app.screen().matches("text:account0")?);
        assert!(!app.screen().matches("id:follow-notices-unread")?);
        app.press("Refresh backend")?;
        app.run_for(Duration::from_millis(300))?;
        app.wait_for("text:reads:1")?;
        app.press("role:button[Sign in]")?;
        app.wait_for("text:reads:2")?;
        assert!(!app.screen().matches("text:account0")?);
        app.wait_for("account1")?;
        Ok(())
    }
}
