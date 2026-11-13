use super::{
    controls::{choices, group},
    directory,
    model::Draft,
    runtime::Runtime,
};
use crate::{
    i18n::{format_bytes, tr},
    ui,
};
use nsh_core::prelude::*;

pub(super) fn section(
    draft: Signal<Draft>,
    busy: Signal<bool>,
    message: Signal<SharedString>,
    external: bool,
    runtime: Runtime,
) -> View {
    group("settings.storage").child(
        view()
            .gap_3()
            .child(
                view()
                    .gap_2()
                    .child(ui::form_label(move || tr("settings.cacheDir")))
                    .child(directory::field(
                        "settings.cacheDir",
                        draft,
                        busy,
                        message,
                        external,
                        |draft| draft.cache_dir.clone(),
                        |draft, value| draft.cache_dir = value,
                    ))
                    .child(ui::form_description(move || tr("settings.cacheDirDesc"))),
            )
            .child(
                text(move || {
                    runtime.usage.with(|usage| {
                        usage
                            .map(|usage| {
                                tr("settings.storageUsage")
                                    .replace("{size}", &format_bytes(usage.downloads.bytes))
                                    .replace("{files}", &usage.downloads.files.to_string())
                            })
                            .unwrap_or_else(|| tr("common.loading").to_string())
                    })
                })
                .text_sm()
                .line_height(px(20.0)),
            )
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(
                        text(move || {
                            runtime.usage.with(|usage| {
                                usage
                                    .map(|usage| {
                                        let cache = &usage.cache;
                                        tr("settings.cacheUsage")
                                            .replace("{size}", &format_bytes(cache.disk_bytes()))
                                            .replace(
                                                "{onlineDisk}",
                                                &format_bytes(cache.online_disk.bytes),
                                            )
                                            .replace("{compat}", &format_bytes(cache.compat.bytes))
                                            .replace("{cover}", &format_bytes(cache.cover.bytes))
                                            .replace("{online}", &cache.online.files.to_string())
                                    })
                                    .unwrap_or_else(|| tr("common.loading").to_string())
                            })
                        })
                        .text_sm()
                        .line_height(px(20.0)),
                    )
                    .child(view().flex_1())
                    .child(
                        ui::library_action(
                            move || {
                                tr(if runtime.clearing.get() {
                                    "common.loading"
                                } else {
                                    "settings.clearCache"
                                })
                            },
                            None,
                        )
                        .disabled(runtime.clearing)
                        .on_press(move || runtime.clear()),
                    ),
            )
            .child(when(
                move || runtime.usage.error().is_some(),
                move || {
                    ui::status(move || {
                        runtime
                            .usage
                            .error()
                            .map(|error| error.text())
                            .unwrap_or_default()
                    })
                },
            ))
            .child(
                view()
                    .gap_2()
                    .child(ui::form_label(move || tr("settings.cacheAutoClean")))
                    .child(
                        view()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap_4()
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        ui::form_label(move || tr("settings.cacheLimit"))
                                            .text_color(move || nsh_base::theme().muted_foreground),
                                    )
                                    .child(
                                        ui::field(move || tr("settings.cacheLimit"))
                                            .w_28()
                                            .keyboard_type(nsh_core::KeyboardType::Number)
                                            .disabled(busy)
                                            .value(move || {
                                                draft.with(|draft| draft.cache_limit.clone())
                                            })
                                            .on_change(move |value: &str| {
                                                draft.update(|draft| {
                                                    draft.cache_limit = value.to_owned()
                                                })
                                            }),
                                    )
                                    .child(ui::form_description(move || {
                                        tr("settings.cacheLimitUnit")
                                    })),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        ui::form_label(move || tr("settings.cacheTtl"))
                                            .text_color(move || nsh_base::theme().muted_foreground),
                                    )
                                    .child(
                                        choices(
                                            "settings.cacheTtl",
                                            draft,
                                            busy,
                                            vec![
                                                (0, "0", "settings.cacheTtlOff"),
                                                (1, "1", "settings.cacheTtlDays1"),
                                                (3, "3", "settings.cacheTtlDays3"),
                                                (7, "7", "settings.cacheTtlDays7"),
                                                (30, "30", "settings.cacheTtlDays30"),
                                            ],
                                            |draft| {
                                                draft
                                                    .cache_ttl
                                                    .parse::<u32>()
                                                    .expect("validated cache duration")
                                            },
                                            |draft, value| draft.cache_ttl = value.to_string(),
                                        )
                                        .w_36(),
                                    ),
                            ),
                    )
                    .child(ui::form_description(move || {
                        tr("settings.cacheAutoCleanDesc")
                    })),
            ),
    )
}
