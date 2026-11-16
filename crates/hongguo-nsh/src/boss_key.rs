use nsh_core::{
    desktop::Shortcut,
    element::desktop::{register_global_shortcut, unregister_global_shortcut},
    prelude::*,
};
use tokio::sync::oneshot;

type RegistrationResult = Result<(), SharedString>;

#[derive(Clone, Copy)]
struct BossKey {
    owner: Owner,
    pending: Stored<bool>,
    hidden: Signal<bool>,
    registered: Signal<String>,
}

pub(crate) fn hidden() -> bool {
    use_context::<BossKey>().is_some_and(|state| state.hidden.get())
}

pub(crate) async fn apply(shortcut: String) -> RegistrationResult {
    let state = use_context::<BossKey>()
        .filter(|_| is_desktop())
        .ok_or_else(|| crate::i18n::tr("settings.bossKeyRegisterFailed"))?;
    if state.pending.replace(true) {
        return Err(crate::i18n::tr("common.loading"));
    }
    let (send, receive) = oneshot::channel();
    state.owner.run(move || {
        spawn(async move {
            let result = state.register(shortcut).await;
            state.pending.replace(false);
            let _ = send.send(result);
        });
    });
    receive
        .await
        .unwrap_or_else(|_| Err(crate::i18n::tr("settings.bossKeyRegisterFailed")))
}

pub(crate) fn install(initial: String) {
    let state = BossKey {
        owner: Owner::current(),
        pending: stored(false),
        hidden: signal(false),
        registered: signal(String::new()),
    };
    provide_context(state);
    if is_desktop() && !initial.is_empty() {
        spawn(async move {
            if let Err(error) = apply(initial).await {
                log::error!("[NSH boss key] startup registration failed: {error}");
            }
        });
    }
}

impl BossKey {
    async fn register(self, spec: String) -> RegistrationResult {
        let parse = |value: &str| {
            if value.trim().is_empty() {
                Ok(None)
            } else {
                Shortcut::parse(value, matches!(os(), nsh_core::platform::Os::MacOs))
                    .map(Some)
                    .map_err(|error| SharedString::from(error.to_string()))
            }
        };
        let next = parse(&spec)?;
        let previous = self.registered.get_untracked();
        if next == parse(&previous)? {
            return Ok(());
        }
        if next.is_some() && !register_global_shortcut(&spec, move || self.pressed()).await {
            unregister_global_shortcut(&spec);
            return Err(crate::i18n::tr("settings.bossKeyRegisterFailed"));
        }
        if !previous.is_empty() {
            unregister_global_shortcut(&previous);
        }
        self.registered.set(spec);
        Ok(())
    }

    fn pressed(self) {
        if let Some(shortcuts) = crate::shortcuts::Shortcuts::current()
            && shortcuts.is_capturing()
        {
            shortcuts.receive_registered(&self.registered.get_untracked());
            return;
        }
        self.hidden.update(|hidden| *hidden = !*hidden);
    }
}
