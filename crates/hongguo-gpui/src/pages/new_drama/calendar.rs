use super::cards::status_badge;
use super::*;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, div, img};

use crate::bridge;

impl NewDramaView {
    pub(super) fn render_calendar_row(
        &self,
        item: &CalendarItem,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self.covers.get(&item.cover).cloned();
        let reserved = item.has_subscribed || self.reserved_local.contains(&item.series_id);
        let card_item = item.clone();
        h_flex()
            .id(ElementId::from(format!("new-cal-{}", item.series_id)))
            .items_center()
            .gap_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .p_3()
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground.opacity(0.3)).shadow_md())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_detail(
                    card_item.series_id.clone(),
                    calendar_prefill(&card_item, reserved),
                    cx,
                );
            }))
            .child(
                div()
                    // Tauri SeriesRowCard：封面 w-20（80）× 3:4（107）
                    .w(gpui_kit::px(80.0))
                    .h(gpui_kit::px(107.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(theme.radius + gpui_kit::px(2.0))
                    .bg(theme.secondary)
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div().size_full().into_any_element(),
                    }),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(SharedString::from(item.title.clone())),
                            )
                            .child(if item.is_online {
                                status_badge(
                                    crate::i18n::t("newDrama.online"),
                                    theme.success.opacity(0.15),
                                    theme.success,
                                    theme.radius,
                                )
                            } else {
                                status_badge(
                                    crate::i18n::t("newDrama.upcoming"),
                                    theme.secondary,
                                    theme.secondary_foreground,
                                    theme.radius,
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .children(item.rec_tags.first().map(|heat| {
                                div()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    // 预约人气走 hot（Tauri 日历行 text-hot 同款）
                                    .text_color(crate::tokens::hot(theme.mode))
                                    .child(SharedString::from(heat.clone()))
                            }))
                            .children((!item.category.is_empty()).then(|| {
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(item.category.clone()))
                            }))
                            .children((item.score > 0.0).then(|| {
                                // Tauri calendar-row：星标 + 数字（不留「分」）
                                h_flex()
                                    .items_center()
                                    .gap(gpui_kit::px(2.0))
                                    .child(
                                        gpui_kit::component::Icon::new(
                                            gpui_kit::assets::IconName::Star,
                                        )
                                        .size_3()
                                        .text_color(crate::tokens::star(theme.mode)),
                                    )
                                    .child(
                                        div().text_color(theme.muted_foreground).child(
                                            SharedString::from(format!("{:.1}", item.score)),
                                        ),
                                    )
                            }))
                            .children((item.episode_cnt > 0).then(|| {
                                // Tauri common.episodeCount =「{count} 集」（带空格）
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(crate::i18n::tf(
                                        "common.episodeCount",
                                        &[("count", &item.episode_cnt.to_string())],
                                    )))
                            })),
                    )
                    .children((!item.description.is_empty()).then(|| {
                        div()
                            .text_xs()
                            .line_height(gpui_kit::relative(1.625))
                            .text_color(theme.muted_foreground.opacity(0.8))
                            .line_clamp(2)
                            .overflow_hidden()
                            .child(SharedString::from(item.description.clone()))
                    })),
            )
            .child(
                v_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1p5()
                    .children((item.publish_time > 0).then(|| {
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(format_publish_time(item.publish_time)))
                    }))
                    .children(if item.is_online {
                        let sid = item.series_id.clone();
                        let item_clone = item.clone();
                        vec![
                            Button::new(ElementId::from(format!(
                                "new-cal-play-{}",
                                item.series_id
                            )))
                            .icon(gpui_kit::assets::IconName::Play)
                            .label(crate::i18n::t("player.play"))
                            .outline()
                            .small()
                            .h_8()
                            .px_2p5()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.play_series(sid.clone(), cx);
                            }))
                            .into_any_element(),
                            Button::new(ElementId::from(format!(
                                "new-cal-detail-{}",
                                item.series_id
                            )))
                            .label(crate::i18n::t("nav.detail.title"))
                            .text()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.open_detail(
                                    item_clone.series_id.clone(),
                                    calendar_prefill(&item_clone, reserved),
                                    cx,
                                );
                            }))
                            .into_any_element(),
                        ]
                    } else {
                        let sid = item.series_id.clone();
                        vec![{
                            let button = Button::new(ElementId::from(format!(
                                "new-cal-reserve-{}",
                                item.series_id
                            )))
                            .icon(gpui_kit::assets::IconName::BellRing)
                            .label(if reserved {
                                crate::i18n::t("reservation.reserved")
                            } else {
                                crate::i18n::t("reservation.action")
                            })
                            .small()
                            .h_8()
                            .px_2p5()
                            .rounded(gpui_kit::px(999.0))
                            .disabled(reserved);
                            let button = if reserved {
                                button.secondary()
                            } else {
                                button.primary()
                            };
                            button
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.reserve(sid.clone(), cx);
                                }))
                                .into_any_element()
                        }]
                    }),
            )
            .into_any_element()
    }

    /// 日历卡预约（未上线行尾动作）。
    fn reserve(&mut self, series_id: String, cx: &mut Context<Self>) {
        if self.reserved_local.contains(&series_id) {
            return;
        }
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid, true, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| match result {
                Ok(()) => {
                    view.reserved_local.insert(series_id);
                    window.push_notification(
                        gpui_kit::component::notification::Notification::success(crate::i18n::t(
                            "reservation.done",
                        )),
                        cx,
                    );
                    cx.notify();
                }
                Err(e) => {
                    log::warn!("[New] 预约失败: {e}");
                    window.push_notification(
                        gpui_kit::component::notification::Notification::error(crate::i18n::tf(
                            "common.failedWith",
                            &[("message", &crate::i18n::app_error_text(&e))],
                        )),
                        cx,
                    );
                }
            });
        })
        .detach();
    }
}

/// UTC 毫秒 → "09-27 12:00"（Tauri formatPublishTime 同款）。
fn format_publish_time(ms: i64) -> String {
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

/// 日历条目 → 详情 prefill（rec_tags 当题材标签，与 Tauri 同口径）。
fn calendar_prefill(item: &CalendarItem, reserved: bool) -> DetailPrefill {
    DetailPrefill {
        title: item.title.clone(),
        cover: item.cover.clone(),
        tags: item
            .rec_tags
            .iter()
            .filter(|t| !t.is_empty())
            .cloned()
            .collect(),
        desc: item.description.clone(),
        reserved,
    }
}
