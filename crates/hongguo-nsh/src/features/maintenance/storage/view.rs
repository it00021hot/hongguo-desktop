use super::{Intent, StorageSnapshot};
use crate::{
    i18n::{format_bytes, tr},
    ui,
};
use nsh_core::prelude::*;

pub(super) fn content(
    data: Memo<Option<StorageSnapshot>>,
    busy: Signal<bool>,
    confirming: Signal<bool>,
    message: Signal<SharedString>,
    dispatch: impl Fn(Intent) + 'static,
) -> View {
    let dispatch = stored(dispatch);
    let filter = signal(String::new());
    let rows = computed(move || {
        data.with(|data| {
            data.as_ref()
                .map(|data| {
                    data.rows
                        .iter()
                        .filter(|row| row.matches(&filter.get()))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    let disabled = computed(move || busy.get() || confirming.get());
    ui::form(
        crate::features::maintenance::presentation::maintenance_page()
            .child(
                crate::features::maintenance::presentation::maintenance_card(
                    view()
                        .gap_1p5()
                        .child(
                            view()
                                .flex_row()
                                .gap_2()
                                .items_center()
                                .child(crate::features::maintenance::presentation::maintenance_icon(crate::features::maintenance::presentation::MaintenanceIcon::Storage))
                                .child(crate::features::maintenance::presentation::maintenance_title(move || tr("settings.storage"))),
                        )
                        .child(crate::features::settings::presentation::form_hint(move || tr("storage.desc"))),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .child(
                            text(move || {
                                data.with(|data| {
                                    data.as_ref().map_or_else(
                                        || tr("common.loading").to_string(),
                                        |data| format_bytes(data.bytes),
                                    )
                                })
                            })
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .tabular_nums(),
                        )
                        .child(when(
                            move || data.with(Option::is_some),
                            move || {
                                text(move || {
                                    data.with(|data| {
                                        data.as_ref()
                                            .map(|data| {
                                                format!("{} {}", data.files, tr("storage.files"))
                                            })
                                            .unwrap_or_default()
                                    })
                                })
                                .text_sm()
                                .font_weight(FontWeight::NORMAL)
                                .text_color(move || nsh_base::theme().muted_foreground)
                            },
                        ))
                        .child(
                            crate::features::maintenance::presentation::maintenance_destructive(move || tr("settings.deleteAll"))
                                .ml(auto())
                                .disabled(disabled)
                                .on_press(move || dispatch.with(|send| send(Intent::All))),
                        ),
                ),
            )
            .child(
                crate::features::maintenance::presentation::maintenance_card(crate::features::maintenance::presentation::maintenance_title(move || tr("storage.bySeries"))).child(
                    view()
                        .gap_3()
                        .child(
                            ui::field(move || tr("player.seriesSearch"))
                                .value(filter)
                                .on_change(move |value: &str| filter.set(value.into())),
                        )
                        .child(when(
                            move || rows.with(Vec::is_empty),
                            move || {
                                view()
                                    .py_8()
                                    .items_center()
                                    .child(crate::features::settings::presentation::form_hint(move || tr("common.empty")))
                            },
                        ))
                        .child(
                            virtual_list(
                                move || rows.with(Vec::len),
                                move |at| {
                                    let row = computed(move || {
                                        rows.with(|rows| {
                                            rows.get(at.index()).cloned().unwrap_or_default()
                                        })
                                    });
                                    crate::features::maintenance::presentation::maintenance_row()
                                        .child(crate::features::maintenance::presentation::maintenance_name(move || {
                                            row.with(|row| row.title.clone())
                                        }))
                                        .child(crate::features::maintenance::presentation::maintenance_badge(
                                            move || {
                                                SharedString::from(
                                                    tr("storage.downloadedEpisodes").replace(
                                                        "{count}",
                                                        &row.with(|row| row.files.to_string()),
                                                    ),
                                                )
                                            },
                                            || crate::features::maintenance::presentation::MaintenanceTone::Secondary,
                                        ))
                                        .child(
                                            crate::features::maintenance::presentation::maintenance_bytes(move || {
                                                row.with(|row| format_bytes(row.bytes))
                                            })
                                            .w_16()
                                            .text_right(),
                                        )
                                        .child(
                                            crate::features::maintenance::presentation::maintenance_icon_action(
                                                move || {
                                                    tr("storage.deleteFilesOf").replace(
                                                        "{title}",
                                                        &row.with(|row| row.title.clone()),
                                                    )
                                                },
                                                crate::features::maintenance::presentation::MaintenanceIcon::Delete,
                                            )
                                            .min_h(move || {
                                                px(if is_desktop() { 32.0 } else { 48.0 })
                                            })
                                            .h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
                                            .disabled(disabled)
                                            .on_press(
                                                move || {
                                                    dispatch.with(|send| {
                                                        let row = row.get_untracked();
                                                        send(Intent::Series(row.id, row.title));
                                                    })
                                                },
                                            ),
                                        )
                                },
                            )
                            .viewport(nsh_core::ListViewport::Parent)
                            .row_gap(px(8.0))
                            .keys(move || {
                                rows.with(|rows| {
                                    rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>()
                                })
                            })
                            .estimated_item_size(move || {
                                px(if is_desktop() { 50.0 } else { 66.0 })
                            }),
                        ),
                ),
            )
            .child(ui::transient_notice(message)),
    )
}
