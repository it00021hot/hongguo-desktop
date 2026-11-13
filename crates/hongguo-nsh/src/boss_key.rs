mod platform;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod registration;
#[cfg(test)]
mod tests;

use nsh_core::prelude::*;
use tokio::sync::oneshot;

type RegistrationResult = Result<(), SharedString>;

#[derive(Clone, Copy)]
struct BossKey {
    request: Signal<(u64, String)>,
    pending: Stored<Option<oneshot::Sender<RegistrationResult>>>,
    hidden: Signal<bool>,
    registered: Signal<String>,
}

pub(crate) fn hidden() -> bool {
    use_context::<BossKey>().is_some_and(|state| state.hidden.get())
}

pub(crate) async fn apply(shortcut: String) -> RegistrationResult {
    if !is_desktop() {
        return Err(crate::i18n::tr("settings.bossKeyRegisterFailed"));
    }
    let Some(state) = use_context::<BossKey>() else {
        return Err(crate::i18n::tr("settings.bossKeyRegisterFailed"));
    };
    let (send, receive) = oneshot::channel();
    let occupied = state.pending.with(|pending| {
        if pending.is_some() {
            true
        } else {
            *pending = Some(send);
            false
        }
    });
    if occupied {
        return Err(crate::i18n::tr("common.loading"));
    }
    state.request.update(|(version, spec)| {
        *version += 1;
        *spec = shortcut;
    });
    receive
        .await
        .unwrap_or_else(|_| Err(crate::i18n::tr("settings.bossKeyRegisterFailed")))
}

pub(crate) fn install(initial: String) -> impl IntoElement {
    let state = BossKey {
        request: signal((0, String::new())),
        pending: stored(None),
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
    view()
        .absolute()
        .size(px(1.0))
        .child(when(is_desktop, move || {
            native("hongguo.window-shortcut")
                .absolute()
                .size(px(1.0))
                .opacity(0.0)
                .pointer_events(PointerEvents::None)
                .prop("registration", move || {
                    let (version, shortcut) = state.request.get();
                    serde_json::json!({"version":version,"shortcut":shortcut})
                })
                .on_native_event(move |event: &nsh_core::element::NativeEvent| state.receive(event))
        }))
}

impl BossKey {
    fn receive(self, event: &nsh_core::element::NativeEvent) {
        match event.name.as_ref() {
            "pressed" => {
                if let Some(shortcuts) = crate::shortcuts::Shortcuts::current()
                    && shortcuts.is_capturing()
                {
                    shortcuts.receive_registered(&self.registered.get_untracked());
                    return;
                }
                self.hidden.update(|hidden| *hidden = !*hidden);
            }
            "registered" => {
                let version = event
                    .data
                    .get("version")
                    .and_then(serde_json::Value::as_u64);
                if version != Some(self.request.get_untracked().0) {
                    return;
                }
                if let Some(send) = self.pending.with(Option::take) {
                    let result = event
                        .data
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .map_or(Ok(()), |error| Err(SharedString::from(error.to_owned())));
                    if result.is_ok() {
                        self.registered.set(self.request.get_untracked().1);
                    }
                    let _ = send.send(result);
                }
            }
            _ => {}
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use platform::register;
