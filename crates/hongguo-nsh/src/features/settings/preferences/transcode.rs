use super::runtime::{self, Runtime};
use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

pub(super) fn section(runtime: Runtime) -> View {
    view()
        .gap_3()
        .child(
            crate::features::settings::presentation::settings_note()
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
                .child(crate::features::settings::presentation::backend_badge(
                    move || runtime::backend_label(runtime.capability.get()),
                    move || match runtime.capability.get() {
                        Some(cap) if cap.platform_hw_encoder => {
                            crate::features::settings::presentation::BackendLevel::Hardware
                        }
                        Some(cap) if cap.platform_encoder => {
                            crate::features::settings::presentation::BackendLevel::Software
                        }
                        _ => crate::features::settings::presentation::BackendLevel::Unavailable,
                    },
                ))
                .child(
                    crate::features::settings::presentation::form_action(
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
        .child(crate::features::settings::presentation::form_description(
            move || tr("settings.transcodeBackendHint"),
        ))
}
