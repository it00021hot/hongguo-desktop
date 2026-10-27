//! 历史行渲染：封面/标题/进度徽标/相对时间/继续播放按钮。

use super::*;

use gpui_kit::base::Button as BaseButton;

impl HistoryView {
    /// 虚拟列表按范围物化行（uniform_list 回调，只有可见区间会跑）。
    ///
    /// 条目槽位等高：行本体恒 120px（96px 封面 + 上下 p_3），加原 v_flex
    /// gap_2 的 8px 间距 = 128px 槽；溢出裁剪防异常文本撑破槽位互相叠压。
    pub(super) fn render_row_range(
        &self,
        range: std::ops::Range<usize>,
        managing: bool,
        cx: &mut Context<Self>,
    ) -> Vec<gpui_kit::AnyElement> {
        let visible = self.visible_indices(cx);
        range
            .filter_map(|slot| {
                let item = self.items.get(*visible.get(slot)?)?;
                let picked = self.selected.contains(&item.series_id);
                Some(
                    div()
                        .h(gpui_kit::px(128.0))
                        .pb(gpui_kit::px(8.0))
                        .overflow_hidden()
                        .child(self.render_row(item, managing, picked, cx))
                        .into_any_element(),
                )
            })
            .collect()
    }

    pub(super) fn render_row(
        &self,
        item: &WatchHistoryItem,
        managing: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = crate::cover_img::card(&item.cover, crate::cover_img::CoverShape::Inline);
        // 行标题（无标题回落「未知剧名」）：行本身的无障碍名、选点提示与可见
        // 标题三处同源，抽一个局部量免得三份分支各写一遍。
        let row_title = if item.title.is_empty() {
            crate::i18n::t("history.unknownTitle")
        } else {
            item.title.clone()
        };
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
        // 整行可点 → 用 base::Button 承担 role/focus/Enter-Space（键盘用户能
        // 走到历史行）。库强制 flex row + 交叉轴居中 + 主轴居中：这里只需把
        // 主轴改回 justify_start（内容左对齐，与原 h_flex 一致）。
        BaseButton::new(ElementId::from(format!(
            "history-row-{}:{}",
            item.series_id, item.updated_at_ms
        )))
        .accessibility_label(row_title.clone())
        .items_center()
        .gap_4()
        .rounded(crate::tokens::radius::CARD)
        .border_1()
        .border_color(if picked { theme.primary } else { theme.border })
        .bg(if picked {
            theme.secondary
        } else {
            theme.group_box
        })
        .p_3()
        .cursor_pointer()
        .when(!managing, |s| {
            // Tauri history 卡 `hover:border-foreground/30` 同款（30% 前景色
            // 的柔和描边；原来吃满前景色，比 Tauri 生硬一档）
            s.hover(|s| s.border_color(theme.foreground.opacity(0.3)))
        })
        .justify_start()
        // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一
        // 截；钉回默认行高才与原 div 像素一致。
        .line_height(gpui_kit::phi())
        // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
        .focus_visible(|s| {
            s.border_color(theme.ring)
                .shadow(crate::tokens::focus_ring(theme))
        })
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
                row_title.clone(),
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
                .rounded(crate::tokens::radius::COVER)
                .bg(theme.secondary)
                .child(cover),
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
                        .child(SharedString::from(row_title.clone())),
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
