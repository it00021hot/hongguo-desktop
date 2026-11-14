use super::*;

use crate::bridge;
use hongguo_common::AppError;

impl super::SettingsView {
    pub(super) fn redetect_capability(&mut self, cx: &mut Context<Self>) {
        if self.redetecting {
            return;
        }
        self.redetecting = true;
        cx.notify();
        let rx = bridge::spawn(async move {
            let capability =
                tokio::task::spawn_blocking(hongguo_media_platform::capability::probe::redetect)
                    .await
                    .map_err(|e| AppError::Internal(format!("能力探测阻塞池任务失败: {e}")))?;
            Ok::<_, AppError>(capability)
        });
        cx.spawn(async move |this, cx| {
            let capability = match rx.await.expect("tokio 桥不会丢弃发送端") {
                Ok(capability) => capability,
                Err(e) => {
                    log::warn!("[Settings] 能力重新探测失败: {e}");
                    let _ = this.update(cx, |view, cx| {
                        view.redetecting = false;
                        cx.notify();
                    });
                    return;
                }
            };
            let _ = this.update(cx, |view, cx| {
                view.redetecting = false;
                view.redetect_done = true;
                view.capability = Some(capability);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn backend_label(&self) -> String {
        match &self.capability {
            None => crate::i18n::t("common.loading"),
            Some(cap) => {
                if cap.platform_hw_encoder {
                    crate::i18n::t("settings.backendHardware")
                } else if cap.platform_encoder {
                    crate::i18n::t("settings.backendSoftware")
                } else {
                    crate::i18n::t("settings.backendUnavailable")
                }
            }
        }
    }

    pub(super) fn redetected_line(&self) -> Option<String> {
        self.redetect_done.then(|| {
            crate::i18n::tf(
                "settings.redetectDone",
                &[("backend", self.backend_label().as_str())],
            )
        })
    }
}
