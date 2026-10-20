//! 预约页行卡：选中切换、单行渲染（封面/标题/标签/动作列）与日期格式化。

use super::*;

impl ReservationView {
    fn toggle_selected(&mut self, series_id: &str, cx: &mut Context<Self>) {
        if !self.selected.remove(series_id) {
            self.selected.insert(series_id.to_string());
        }
        cx.notify();
    }

    pub(super) fn render_row(
        &self,
        item: &CalendarItem,
        managing: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self.covers.get(&item.series_id).cloned();
        let publish_date = if item.publish_time > 0 {
            format_month_day(item.publish_time)
        } else {
            String::new()
        };
        let tags: Vec<String> = item
            .rec_tags
            .iter()
            .filter(|t| !t.is_empty())
            .take(3)
            .cloned()
            .collect();
        h_flex()
            .id(ElementId::from(format!(
                "reservation-row-{}",
                item.series_id
            )))
            .items_stretch()
            .gap_4()
            .rounded(gpui_kit::px(12.0))
            .border_1()
            .border_color(if picked { theme.primary } else { theme.border })
            .bg(if picked {
                theme.secondary
            } else {
                theme.group_box
            })
            .p_3()
            .children(managing.then(|| {
                let sid = item.series_id.clone();
                div().flex().items_center().child(pick_dot(
                    ElementId::from(format!("reservation-pick-{}", item.series_id)),
                    picked,
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&sid, cx)),
                    cx,
                ))
            }))
            // 封面（左下上线日期角标）
            .child(
                div()
                    .id(ElementId::from(format!(
                        "reservation-cover-{}",
                        item.series_id
                    )))
                    .relative()
                    .w(gpui_kit::px(92.0))
                    .h(gpui_kit::px(123.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(8.0))
                    .bg(theme.secondary)
                    .cursor_pointer()
                    .on_click({
                        let item = item.clone();
                        cx.listener(move |this, _, _, cx| {
                            if this.managing {
                                this.toggle_selected(&item.series_id, cx);
                            } else {
                                this.open_detail(&item, cx);
                            }
                        })
                    })
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => h_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Tv)
                                    .text_color(theme.muted_foreground),
                            )
                            .into_any_element(),
                    })
                    .children((!publish_date.is_empty()).then(|| {
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .bg(gpui_kit::rgba(0x000000a6))
                            .px_1p5()
                            .py_0p5()
                            .text_size(gpui_kit::px(10.0))
                            .text_color(gpui_kit::rgb(0xffffffff))
                            .child(SharedString::from(crate::i18n::tf(
                                "reservation.publishAt",
                                &[("time", publish_date.as_str())],
                            )))
                    })),
            )
            // 标题/简介/标签
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .justify_center()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id(ElementId::from(format!(
                                        "reservation-title-{}",
                                        item.series_id
                                    )))
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .cursor_pointer()
                                    .on_click({
                                        let item = item.clone();
                                        cx.listener(move |this, _, _, cx| {
                                            if this.managing {
                                                this.toggle_selected(&item.series_id, cx);
                                            } else {
                                                this.open_detail(&item, cx);
                                            }
                                        })
                                    })
                                    .child(SharedString::from(item.title.clone())),
                            )
                            .children((item.score > 0.0).then(|| {
                                h_flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .gap_0p5()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(
                                        gpui_kit::component::Icon::new(
                                            gpui_kit::assets::IconName::Star,
                                        )
                                        .small()
                                        .text_color(crate::tokens::star(theme.mode)),
                                    )
                                    .child(SharedString::from(format!("{:.1}", item.score)))
                            })),
                    )
                    .children((!item.description.is_empty()).then(|| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(item.description.clone()))
                    }))
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1p5()
                            .mt_1()
                            .children((!item.category.is_empty()).then(|| {
                                div()
                                    .rounded(gpui_kit::px(4.0))
                                    .bg(theme.secondary)
                                    .px_1p5()
                                    .py_0p5()
                                    .text_size(gpui_kit::px(10.0))
                                    .child(SharedString::from(item.category.clone()))
                            }))
                            .children(tags.into_iter().map(|tag| {
                                div()
                                    .rounded(gpui_kit::px(4.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .px_1p5()
                                    .py_0p5()
                                    .text_size(gpui_kit::px(10.0))
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(tag))
                            }))
                            .children((item.publish_time > 0 && !item.is_online).then(|| {
                                div().text_xs().text_color(theme.muted_foreground).child(
                                    SharedString::from(crate::i18n::tf(
                                        "reservation.publishAt",
                                        &[(
                                            "time",
                                            format_month_day_time(item.publish_time).as_str(),
                                        )],
                                    )),
                                )
                            })),
                    ),
            )
            // 动作列（管理态不渲染）
            .children((!managing).then(|| {
                let mut column = v_flex().flex_shrink_0().justify_center().gap_1p5();
                if item.is_online {
                    let sid = item.series_id.clone();
                    column = column
                        .child(
                            Button::new(ElementId::from(format!(
                                "reservation-play-{}",
                                item.series_id
                            )))
                            .icon(gpui_kit::assets::IconName::Play)
                            .label(crate::i18n::t("player.play"))
                            .outline()
                            .small()
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.play_series(&sid, cx);
                                },
                            )),
                        )
                        .child({
                            let item = item.clone();
                            Button::new(ElementId::from(format!(
                                "reservation-detail-{}",
                                item.series_id
                            )))
                            .label(crate::i18n::t("nav.detail.title"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.open_detail(&item, cx);
                                },
                            ))
                        });
                } else {
                    let sid = item.series_id.clone();
                    let cancelling = self.cancelling.contains(&item.series_id);
                    column = column.child(
                        Button::new(ElementId::from(format!(
                            "reservation-cancel-{}",
                            item.series_id
                        )))
                        .icon(gpui_kit::assets::IconName::BellRing)
                        .label(crate::i18n::t("reservation.cancel"))
                        .outline()
                        .small()
                        .disabled(cancelling)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cancel_reserve(sid.clone(), cx);
                        })),
                    );
                }
                column
            }))
            .into_any_element()
    }
}

/// UTC 毫秒 → "9月27日"（角标用）。
fn format_month_day(ms: i64) -> String {
    use chrono::{Datelike, TimeZone};
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(dt) => match crate::i18n::locale() {
            crate::i18n::Locale::ZhCn => format!("{}月{}日", dt.month(), dt.day()),
            crate::i18n::Locale::EnUs => format!("{:02}/{:02}", dt.month(), dt.day()),
        },
        None => String::new(),
    }
}

/// UTC 毫秒 → "10-07 12:00"（待上线精确时间）。
fn format_month_day_time(ms: i64) -> String {
    use chrono::{Datelike, TimeZone, Timelike};
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(dt) => format!(
            "{:02}-{:02} {:02}:{:02}",
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute()
        ),
        None => String::new(),
    }
}
