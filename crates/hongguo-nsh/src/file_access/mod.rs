use nsh_core::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::oneshot;

#[cfg(test)]
mod tests;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub(crate) use ios::register;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ExportRequest {
    serial: u32,
    path: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportResponse {
    serial: u32,
    is_cancelled: bool,
    error: Option<String>,
}

type Completion = oneshot::Sender<Result<bool, String>>;

#[derive(Clone, Copy)]
pub(crate) struct FileAccess {
    request: Signal<Option<ExportRequest>>,
    completion: Stored<Option<Completion>>,
}

impl FileAccess {
    pub(crate) fn current() -> Option<Self> {
        use_context()
    }

    async fn export(self, path: PathBuf) -> Result<bool, String> {
        if self.completion.with(|completion| completion.is_some()) {
            return Err("A file export is already in progress".into());
        }
        let path = path
            .to_str()
            .ok_or_else(|| "The export path is not valid UTF-8".to_owned())?
            .to_owned();
        let serial = self.request.with_untracked(|request| {
            request
                .as_ref()
                .map_or(1, |request| request.serial.wrapping_add(1))
        });
        let (sender, receiver) = oneshot::channel();
        self.completion.replace(Some(sender));
        self.request.set(Some(ExportRequest { serial, path }));
        receiver
            .await
            .map_err(|_| "The file export host was closed".to_owned())?
    }

    fn receive(self, response: ExportResponse) {
        if self
            .request
            .with_untracked(|request| request.as_ref().map(|request| request.serial))
            != Some(response.serial)
        {
            return;
        }
        if let Some(sender) = self.completion.with(|completion| completion.take()) {
            let result = response.error.map_or(Ok(!response.is_cancelled), Err);
            let _ = sender.send(result);
        }
    }
}

pub(crate) async fn reveal_or_export(
    access: Option<FileAccess>,
    path: PathBuf,
) -> Result<SharedString, String> {
    if is_desktop() {
        reveal_path(path).await?;
        return Ok(SharedString::default());
    }
    let access = access.ok_or_else(|| "The file export host is unavailable".to_owned())?;
    Ok(if access.export(path).await? {
        crate::i18n::tr("nsh.exportCompleted")
    } else {
        SharedString::default()
    })
}

pub(crate) fn install() -> impl IntoElement {
    let access = FileAccess {
        request: signal(None),
        completion: stored(None),
    };
    provide_context(access);
    view()
        .absolute()
        .size(px(1.0))
        .opacity(0.0)
        .pointer_events(PointerEvents::None)
        .child(when(
            move || !is_desktop(),
            move || {
                native("hongguo.file-export")
                    .size(px(1.0))
                    .prop("request", move || {
                        serde_json::to_value(access.request.get()).expect("file export request")
                    })
                    .on_native_event(move |event: &nsh_core::element::NativeEvent| {
                        if event.name.as_ref() != "completed" {
                            return;
                        }
                        match serde_json::from_value::<ExportResponse>(event.data.clone()) {
                            Ok(response) => access.receive(response),
                            Err(error) => {
                                if let Some(sender) =
                                    access.completion.with(|completion| completion.take())
                                {
                                    let _ = sender.send(Err(format!(
                                        "Invalid file export response: {error}"
                                    )));
                                }
                            }
                        }
                    })
            },
        ))
}
