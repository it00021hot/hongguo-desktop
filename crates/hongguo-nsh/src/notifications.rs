mod platform;
#[cfg(test)]
mod tests;
mod wire;

use nsh_core::prelude::*;
use std::collections::BTreeMap;
use tokio::sync::oneshot;
use wire::{Action, Request, Response};

const MAX_PENDING: usize = 32;
type Pending = BTreeMap<u32, oneshot::Sender<Result<bool, String>>>;

#[derive(Clone, Copy)]
pub(crate) struct Notifications {
    serial: Signal<u32>,
    requests: Signal<Vec<Request>>,
    pending: Stored<Pending>,
}

pub(crate) fn install() -> impl IntoElement {
    let state = Notifications {
        serial: signal(0),
        requests: signal(Vec::new()),
        pending: stored(BTreeMap::new()),
    };
    provide_context(state);
    native("hongguo.notifications")
        .absolute()
        .size(px(1.0))
        .opacity(0.0)
        .pointer_events(PointerEvents::None)
        .prop("requests", move || {
            serde_json::to_value(state.requests.get()).expect("notification request serialization")
        })
        .on_native_event(move |event: &nsh_core::element::NativeEvent| {
            if event.name.as_ref() == "activated" {
                nsh_core::element::desktop::current_window().focus();
                return;
            }
            if event.name.as_ref() != "result" {
                return;
            }
            match serde_json::from_value::<Response>(event.data.clone()) {
                Ok(response) => state.receive(response),
                Err(error) => log::error!("[NSH notifications] invalid native response: {error}"),
            }
        })
}

impl Notifications {
    pub(crate) fn current() -> Self {
        use_context::<Self>().expect("notifications installed before the session")
    }

    pub(crate) async fn request_permission(self) -> Result<bool, String> {
        self.request(Action::RequestPermission).await
    }

    pub(crate) async fn send(self, id: &str, title: &str, body: &str) -> Result<(), String> {
        match self
            .request(Action::Send {
                id: id.into(),
                title: title.into(),
                body: body.into(),
            })
            .await?
        {
            true => Ok(()),
            false => Err("System notifications are disabled".into()),
        }
    }

    async fn request(self, action: Action) -> Result<bool, String> {
        let state = self;
        if state.pending.with(|pending| pending.len()) >= MAX_PENDING {
            return Err("Too many pending notification requests".into());
        }
        let (send, receive) = oneshot::channel();
        state.serial.update(|serial| *serial += 1);
        let serial = state.serial.get_untracked();
        state.pending.with(|pending| pending.insert(serial, send));
        state
            .requests
            .update(|requests| requests.push(Request { serial, action }));
        receive
            .await
            .map_err(|_| "Notification host was detached".to_owned())?
    }

    fn receive(self, response: Response) {
        if let Some(send) = self
            .pending
            .with(|pending| pending.remove(&response.serial))
        {
            self.requests
                .update(|requests| requests.retain(|request| request.serial != response.serial));
            let result = response.error.map_or(Ok(response.granted), Err);
            let _ = send.send(result);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
pub(crate) use platform::register;
