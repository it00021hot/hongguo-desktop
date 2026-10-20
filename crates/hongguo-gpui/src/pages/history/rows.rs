//! 历史行渲染：封面/标题/进度徽标/相对时间/继续播放按钮。

use super::*;

impl HistoryView {
    pub(super) fn render_row(
        &self,
        item: &WatchHistoryItem,
        managing: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self
            .covers
            .get(&format!("{}:{}", item.series_id, item.updated_at_ms))
            .cloned();
        let index = item.vid_index.max(1);
        let badge = if item.duration_ms > 0 {
            let total = if item.episode_cnt > 0 {
                item.episode_cnt.to_string()
            } else {
                "–".to_string()
            };
            crate::i18n::tf(
                "history.watching",
                &[
                    ("index", &index.to_string()),
                    ("total", total.as_str()),
                    (
                        "time",
                        format_duration(item.position_ms as f64 / 1000.0).as_str(),
                    ),
                ],
            )
        } else {
            crate::i18n::tf("player.epShort", &[("index", &index.to_string())])
        };
        h_flex()
            .id(ElementId::from(format!(
                "history-row-{}:{}",
                item.series_id, item.updated_at_ms
            )))
            .items_center()
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
            .cursor_pointer()
            .when(!managing, |s| s.hover(|s| s.border_color(theme.foreground)))
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| {
                    if this.managing {
                        this.toggle_selected(&item.series_id, cx);
                    } else {
                        this.open_item(&item, cx);
                    }
                })
            })
            .children(managing.then(|| {
                let sid = item.series_id.clone();
                pick_dot(
                    ElementId::from(format!("history-pick-{}", item.series_id)),
                    picked,
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&sid, cx)),
                    cx,
                )
            }))
            .child(
                div()
                    .w(gpui_kit::px(72.0))
                    .h(gpui_kit::px(96.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(8.0))
                    .bg(theme.secondary)
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
                                gpui_kit::component::Icon::new(
                                    gpui_kit::assets::IconName::ClockArrowDown,
                                )
                                .text_color(theme.muted_foreground),
                            )
                            .into_any_element(),
                    }),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1p5()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(SharedString::from(if item.title.is_empty() {
                                crate::i18n::t("history.unknownTitle")
                            } else {
                                item.title.clone()
                            })),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .rounded(gpui_kit::px(4.0))
                                    .bg(theme.secondary)
                                    .px_1p5()
                                    .py_0p5()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child(SharedString::from(badge)),
                            )
                            .child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(days_ago(item.updated_at_ms))),
                            ),
                    ),
            )
            .children((!managing).then(|| {
                let item = item.clone();
                Button::new(ElementId::from(format!(
                    "history-continue-{}:{}",
                    item.series_id, item.updated_at_ms
                )))
                .icon(gpui_kit::assets::IconName::Play)
                .label(crate::i18n::t("history.continue"))
                .outline()
                .small()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_item(&item, cx);
                }))
            }))
            .into_any_element()
    }
}

/// unix 毫秒 → 相对时间（今天/昨天/N 天前，Tauri history formatTimeAgo 同款）。
fn days_ago(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let diff_days = (now - ms) / 86_400_000;
    match diff_days {
        d if d <= 0 => crate::i18n::t("history.today"),
        1 => crate::i18n::t("history.yesterday"),
        d => crate::i18n::tf("history.daysAgo", &[("count", &d.to_string())]),
    }
}
