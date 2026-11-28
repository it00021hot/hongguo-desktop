use super::model::Draft;
use crate::{bridge, events::SessionHandle, i18n::tr};
use hongguo_common::model::Settings;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
struct Updates {
    session: SessionHandle,
    draft: Signal<Draft>,
    observed: Stored<Draft>,
    validation: Signal<SharedString>,
    message: Signal<SharedString>,
    page: Owner,
}

pub(super) fn install(
    session: SessionHandle,
    draft: Signal<Draft>,
    validation: Signal<SharedString>,
    message: Signal<SharedString>,
) {
    let updates = Updates {
        session,
        draft,
        observed: stored(draft.get_untracked()),
        validation,
        message,
        page: Owner::current(),
    };
    let tasks = super::super::Tasks::provide();
    effect(move || {
        let next = draft.get();
        let previous = updates.observed.replace(next.clone());
        if next == previous {
            return;
        }
        untracked(move || {
            validation.set(
                next.invalid_number()
                    .map_or_else(SharedString::default, number_error),
            );
            let gate = tasks.updates.get();
            tasks.owner.run(move || {
                spawn(async move {
                    let _guard = gate.lock().await;
                    updates.apply(previous, next).await;
                });
            });
        });
    });
}

impl Updates {
    async fn apply(self, previous: Draft, next: Draft) {
        let state = self.session.with(|session| session.state.clone());
        let desktop = is_desktop();
        let proposed = match validate(&previous, &next, state.settings(), desktop) {
            Ok(settings) => settings,
            Err(error) => {
                if self.page.is_live() && self.draft.get_untracked() == next {
                    self.validation.set(error);
                }
                return;
            }
        };
        let old_boss = state.settings().boss_key;
        let boss_changed = desktop && proposed.boss_key != old_boss;
        if boss_changed && let Err(error) = crate::boss_key::apply(proposed.boss_key).await {
            self.restore(&previous, &next);
            self.report(error);
            return;
        }
        let before = previous.clone();
        let after = next.clone();
        let result = bridge::request(async move {
            hongguo_service::runtime::spawn_blocking(move || {
                let settings =
                    after
                        .apply(&before, state.settings(), desktop)
                        .map_err(|error| {
                            hongguo_common::AppError::InvalidArgs(format!(
                                "{}: {}–{}",
                                error.field, error.minimum, error.maximum
                            ))
                        })?;
                hongguo_service::settings_service::save(&state, settings)
            })
            .await
            .map_err(|error| hongguo_common::AppError::Internal(error.to_string()))?
        })
        .await;
        match result {
            Ok(_) => self.session.notify_identity(),
            Err(error) => {
                let mut notice = error.text();
                if boss_changed && let Err(rollback) = crate::boss_key::apply(old_boss).await {
                    log::error!(
                        "[NSH settings] restoring the previous global shortcut failed: {rollback}"
                    );
                    notice = format!(
                        "{notice}\n{}",
                        tr("nsh.bossKeyRollbackFailed").replace("{error}", &rollback)
                    )
                    .into();
                }
                self.restore(&previous, &next);
                self.report(notice);
            }
        }
    }

    fn restore(self, previous: &Draft, attempted: &Draft) {
        if !self.page.is_live() {
            return;
        }
        let saved = Draft::from(self.session.with(|session| session.state.settings()));
        let mut current = self.draft.get_untracked();
        current.restore_changed(previous, attempted, saved);
        self.observed.replace(current.clone());
        self.draft.set(current);
    }

    fn report(self, error: SharedString) {
        log::error!("[NSH settings] applying setting failed: {error}");
        if self.page.is_live() {
            self.message.set(error);
        }
    }
}

fn validate(
    previous: &Draft,
    next: &Draft,
    latest: Settings,
    desktop: bool,
) -> Result<Settings, SharedString> {
    let settings = next
        .clone()
        .apply(previous, latest, desktop)
        .map_err(number_error)?;
    if desktop
        && previous.boss_key != next.boss_key
        && crate::shortcuts::Shortcuts::current()
            .is_some_and(|keys| !keys.conflicts(&settings.boss_key).is_empty())
    {
        return Err(tr("settings.bossKeyConflictBlock"));
    }
    Ok(settings)
}

fn number_error(error: super::model::InvalidNumber) -> SharedString {
    format!("{}: {}–{}", tr(error.field), error.minimum, error.maximum).into()
}
