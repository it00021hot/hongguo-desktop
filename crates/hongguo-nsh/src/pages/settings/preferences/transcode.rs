use super::runtime::{self, Runtime};
use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

pub(super) fn section(runtime: Runtime, message: Signal<SharedString>, external: bool) -> View {
    view()
        .gap_3()
        .child(
            ui::settings_note()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    ui::icon(ui::Icon::Cpu).text_color(move || nsh_base::theme().muted_foreground),
                )
                .child(
                    text(move || tr("settings.transcodeBackend"))
                        .text_sm()
                        .line_height(px(20.0)),
                )
                .child(view().flex_1())
                .child(ui::backend_badge(
                    move || runtime::backend_label(runtime.capability.get()),
                    move || match runtime.capability.get() {
                        Some(cap) if cap.platform_hw_encoder || cap.hevc_hw_encoder => {
                            ui::BackendLevel::Hardware
                        }
                        Some(cap) if cap.platform_encoder || cap.has_ffmpeg => {
                            ui::BackendLevel::Software
                        }
                        _ => ui::BackendLevel::Unavailable,
                    },
                ))
                .child(
                    ui::form_action(
                        move || {
                            tr(if runtime.probing.get() {
                                "common.loading"
                            } else {
                                "settings.redetectBackend"
                            })
                        },
                        ui::ButtonVariant::Quiet,
                    )
                    .disabled(runtime.probing)
                    .on_press(move || runtime.redetect()),
                ),
        )
        .child(ui::form_description(move || {
            tr("settings.transcodeBackendHint")
        }))
        .child(when(
            move || {
                is_desktop()
                    && runtime
                        .capability
                        .get()
                        .is_some_and(|cap| !cap.platform_encoder && !cap.has_ffmpeg)
            },
            move || install_help(message, external),
        ))
}

fn install_help(message: Signal<SharedString>, external: bool) -> View {
    let (command, url) = if os() == nsh_core::platform::Os::MacOs {
        ("brew install ffmpeg", "https://ffmpeg.org/download.html")
    } else {
        (
            "winget install Gyan.FFmpeg",
            "https://www.gyan.dev/ffmpeg/builds/",
        )
    };
    ui::settings_note()
        .child(ui::form_description(move || {
            tr("settings.ffmpegInstallHint")
        }))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .items_center()
                .child(
                    text(command)
                        .font_family("monospace")
                        .text_xs()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(move || nsh_base::theme().background),
                )
                .child(
                    ui::settings_glyph_action(move || tr("settings.copyCommand"), ui::Icon::Copy)
                        .on_press(move || {
                            if external {
                                set_clipboard(command);
                                message.set(tr("settings.commandCopied"));
                            } else {
                                message.set(
                                    tr("nsh.previewAction")
                                        .replace("{action}", &tr("settings.copyCommand"))
                                        .into(),
                                );
                            }
                        }),
                )
                .child(
                    ui::settings_glyph_action(
                        move || tr("settings.ffmpegDownloadPage"),
                        ui::Icon::ExternalLink,
                    )
                    .on_press(move || {
                        if !external {
                            message.set(
                                tr("nsh.previewAction")
                                    .replace("{action}", &tr("settings.ffmpegDownloadPage"))
                                    .into(),
                            );
                            return;
                        }
                        spawn(async move {
                            if !open_url(url).await {
                                message.set(tr("error.network"));
                            }
                        });
                    }),
                ),
        )
        .child(ui::form_description(move || {
            tr("settings.ffmpegInstallNote")
        }))
}
