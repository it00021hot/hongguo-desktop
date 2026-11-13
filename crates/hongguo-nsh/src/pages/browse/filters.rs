use super::model::{Dimension, Query, rows_with_fallback};
use crate::{bridge, i18n::tr, ui};
use hongguo_api::discover::SelectorRow;
use nsh_core::prelude::*;

pub(super) type Panel = Resource<Vec<SelectorRow>, bridge::Failure>;

pub(super) fn content(panel: Panel, query: Signal<Query>) -> View {
    view()
        .gap_1p5()
        .child(when(move || panel.loading(), ui::browse_panel_loading))
        .child(when(
            move || panel.error().is_some(),
            move || {
                crate::pages::error_state(
                    move || {
                        panel
                            .error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || panel.refetch(),
                )
            },
        ))
        .child(when(
            move || !panel.loading() && panel.error().is_none(),
            move || {
                each(
                    move || {
                        rows_with_fallback(panel.get().unwrap_or_default(), |key| {
                            tr(key).to_string()
                        })
                        .into_iter()
                        .filter(|row| Dimension::from_row(&row.row_type).is_some())
                        .collect::<Vec<_>>()
                    },
                    |row: &SelectorRow| row.row_type.clone(),
                    move |row, _| {
                        let dimension = Dimension::from_row(&row.get_untracked().row_type)
                            .expect("known filter dimension");
                        view()
                            .role(nsh_core::AccessibilityRole::Group)
                            .label(move || tr(dimension.label()))
                            .flex_row()
                            .items_start()
                            .gap_3()
                            .child(
                                text(move || tr(dimension.label()))
                                    .w_10()
                                    .flex_shrink_0()
                                    .pt_1()
                                    .text_xs()
                                    .line_height(px(16.0))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(move || nsh_base::theme().muted_foreground),
                            )
                            .child(
                                ui::browse_filter_group()
                                    .child(
                                        ui::browse_chip(move || tr("browse.all"))
                                            .pressed(move || {
                                                query.with(|query| {
                                                    dimension.get(&query.filters).is_empty()
                                                })
                                            })
                                            .on_change(move |_: &bool| {
                                                query.update(|q| q.pick(dimension, String::new()));
                                            }),
                                    )
                                    .child(each(
                                        move || row.get().items,
                                        |item: &hongguo_api::discover::SelectorItem| {
                                            item.id.clone()
                                        },
                                        move |item, _| {
                                            ui::browse_chip(move || {
                                                SharedString::from(item.get().name)
                                            })
                                            .pressed(move || {
                                                query.with(|q| {
                                                    dimension.get(&q.filters)
                                                        == item.with(|item| item.id.clone())
                                                })
                                            })
                                            .on_change(move |pressed: &bool| {
                                                let value = if *pressed {
                                                    item.get_untracked().id
                                                } else {
                                                    String::new()
                                                };
                                                query.update(|q| q.pick(dimension, value));
                                            })
                                        },
                                    )),
                            )
                    },
                )
            },
        ))
}
