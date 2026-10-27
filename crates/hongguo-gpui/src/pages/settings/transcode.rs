//! 转码/解码能力探测：后端标签、重新探测与 ffmpeg 安装指引（含用系统
//! 默认方式打开下载页）。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::{ClipboardItem, div};

use crate::bridge;
use crate::ui_button::button as Button;
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
                if cap.platform_hw_encoder || cap.h264_hw_encoder {
                    crate::i18n::t("settings.backendFfmpegHw")
                } else if cap.platform_encoder || cap.has_ffmpeg {
                    crate::i18n::t("settings.backendFfmpegSw")
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

/// ffmpeg 安装指引块（未装 ffmpeg 时出现；命令按平台）。
pub(super) fn ffmpeg_install_block(cx: &mut Context<SettingsView>) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    #[cfg(target_os = "macos")]
    let (cmd, page): (&str, &str) = ("brew install ffmpeg", "https://ffmpeg.org/download.html");
    #[cfg(not(target_os = "macos"))]
    let (cmd, page): (&str, &str) = (
        "winget install Gyan.FFmpeg",
        "https://www.gyan.dev/ffmpeg/builds/",
    );
    v_flex()
        .w_full()
        .gap_2()
        .rounded(gpui_kit::px(8.0))
        .bg(theme.secondary)
        .p_3()
        .text_xs()
        .child(div().child(crate::i18n::t("settings.ffmpegInstallHint")))
        .child(
            h_flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .rounded(gpui_kit::px(4.0))
                        .bg(theme.background)
                        .px_2()
                        .py_1()
                        .font_family(theme.mono_font_family.clone())
                        .child(cmd),
                )
                .child(
                    Button::new("settings-copy-ffmpeg")
                        .icon(gpui_kit::assets::IconName::Copy)
                        .label(crate::i18n::t("settings.copyCommand"))
                        .ghost()
                        .small()
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(cmd.to_string()));
                        }),
                )
                .child(
                    Button::new("settings-open-ffmpeg")
                        .icon(gpui_kit::assets::IconName::ExternalLink)
                        .label(crate::i18n::t("settings.ffmpegDownloadPage"))
                        .ghost()
                        .small()
                        .on_click(move |_, _, _| open_external(page)),
                ),
        )
        .child(div().child(crate::i18n::t("settings.ffmpegInstallNote")))
        .into_any_element()
}

/// 用系统默认方式打开外部页面（Tauri openExternalPage 的 GPUI 对应；
/// 调用点固定为设置页的白名单地址）。
fn open_external(url: &str) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = {
        use std::os::windows::process::CommandExt;
        // cmd.exe 是控制台程序：GUI 进程里不压 CREATE_NO_WINDOW 会闪黑窗
        std::process::Command::new("cmd")
            .raw_arg(format!("/c start \"\" \"{url}\""))
            .creation_flags(0x0800_0000)
            .spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result: std::io::Result<std::process::Child> = {
        log::warn!("[Settings] 打开外部页面在 {url} 所在平台未实现");
        return;
    };
    if let Err(e) = result {
        log::warn!("[Settings] 打开外部页面失败: {e}");
    }
}
