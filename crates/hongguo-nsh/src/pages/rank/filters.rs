use super::model::Query;
use crate::{i18n::tr, ui};
use hongguo_api::rank::{RankPanelItem, RankPanelRow, RankSubList};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Month,
    Total,
}

pub(super) fn control(
    current: Memo<Option<RankSubList>>,
    query: Signal<Query>,
    kind: Kind,
) -> View {
    let open = signal(false);
    let rows = computed(move || {
        current
            .get()
            .map(|sub| match kind {
                Kind::Month => sub.background,
                Kind::Total => sub.panel,
            })
            .unwrap_or_default()
    });
    let value = computed(move || {
        query.with(|query| match kind {
            Kind::Month => query.background.clone(),
            Kind::Total => query.panel.clone(),
        })
    });
    let pick = move |id: String| {
        query.update(|query| {
            match kind {
                Kind::Month => query.background = id,
                Kind::Total => query.panel = id,
            }
            query.reset_paging();
        });
        open.set(false);
    };
    let label = computed(move || {
        rows.with(|rows| {
            rows.iter()
                .flat_map(|row| &row.items)
                .find(|item| !item.id.is_empty() && item.id == value.get())
                .map(|item| SharedString::from(item.name.clone()))
                .unwrap_or_else(|| {
                    tr(match kind {
                        Kind::Month => "rank.filter.month",
                        Kind::Total => "rank.filter.all",
                    })
                })
        })
    });
    ui::rank_filter_panel(
        open,
        ui::rank_filter(label, open).disabled(move || rows.with(Vec::is_empty)),
        move || {
            view().min_h(px(0.0)).flex_shrink(1.0).child(
                ui::rank_filter_scroll()
                    .test_id(match kind {
                        Kind::Month => "rank-month-filter",
                        Kind::Total => "rank-total-filter",
                    })
                    .child(
                        view()
                            .gap_2()
                            .child(
                                view()
                                    .flex_row()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        text(move || tr("rank.filter.title"))
                                            .text_sm()
                                            .line_height(px(20.0))
                                            .font_weight(FontWeight::MEDIUM),
                                    )
                                    .child(
                                        ui::rank_filter_close().on_press(move || open.set(false)),
                                    ),
                            )
                            .child(when(
                                move || !value.get().is_empty(),
                                move || {
                                    ui::button(
                                        move || tr("rank.filter.reset"),
                                        ui::ButtonVariant::Quiet,
                                    )
                                    .self_end()
                                    .on_press(move || pick(String::new()))
                                },
                            ))
                            .child(each(
                                move || rows.get(),
                                |row: &RankPanelRow| row.name.clone(),
                                move |row, _| {
                                    view()
                                        .gap_1p5()
                                        .child(
                                            text(move || row.get().name)
                                                .text_xs()
                                                .font_weight(FontWeight::NORMAL)
                                                .text_color(move || {
                                                    nsh_base::theme().muted_foreground
                                                }),
                                        )
                                        .child(view().flex_row().flex_wrap().gap_1p5().child(each(
                                            move || row.get().items,
                                            |item: &RankPanelItem| item.id.clone(),
                                            move |item, _| {
                                                ui::rank_filter_option(move || {
                                                    SharedString::from(item.get().name)
                                                })
                                                .selected(move || item.get().id == value.get())
                                                .on_press(move || pick(item.get_untracked().id))
                                            },
                                        )))
                                },
                            )),
                    ),
            )
        },
    )
    .style_when(move || rows.with(Vec::is_empty), |style| style.hidden())
}
