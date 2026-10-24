//! 找剧页结果卡：浏览流与搜索结果共用的卡片渲染。

use super::*;

use gpui_kit::base::Button as BaseButton;

impl super::BrowseView {
    /// 一张结果卡（浏览/搜索共用；字段差异在调用方已归一）。
    ///
    /// 卡宽不在这儿定：卡片是网格的一槽，等宽由 [`crate::pages::grid::row`]
    /// 给（与 Tauri CSS grid 的 `1fr` 列同解）；封面按 3:4 由
    /// [`crate::pages::grid::cover_box`] 定（同 Tauri `aspect-[3/4]`）。
    pub(super) fn render_card(
        &self,
        card: CardView<'_>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::base::Button {
        let CardView {
            series_id,
            title,
            cover_url,
            episode_count,
            heat,
            tags,
        } = card;
        let theme = cx.theme();
        let downloaded = self.downloaded.get(series_id).copied().unwrap_or(0);
        let tags: Vec<String> = tags
            .iter()
            .filter(|t| !t.is_empty())
            .take(3)
            .cloned()
            .collect();
        // 整卡可点 → 用 base::Button 承担 role/focus/Enter-Space（Tauri 侧是
        // `article role="button" tabIndex={0}`，同一语义）。库强制 flex row +
        // 居中，这里要复原原 v_flex 的竖排：flex_col + items_stretch（子件
        // 撑满卡宽，封面才按卡宽裁）+ justify_start（内容从顶上排）。
        BaseButton::new(ElementId::from(format!("browse-card-{series_id}")))
            .accessibility_label(title.to_string())
            // 卡宽跟着网格列走（Tauri `w-full` + 响应式列数）：宽度由网格槽
            // 给（`grid::row`），不在卡上写死——写死就要把侧栏宽与页面内边距
            // 一起硬编码，侧栏一展开就算错。
            .overflow_hidden()
            .rounded(crate::tokens::radius::CARD)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground))
            .flex_col()
            .items_stretch()
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
                let series_id = series_id.to_string();
                cx.listener(move |this, _, _, cx| {
                    this.play_series(series_id.clone(), cx);
                })
            })
            .child(
                crate::pages::grid::cover_box()
                    // 封面顶两角跟着卡片圆角（Tauri：卡片 overflow-hidden +
                    // rounded-xl 把封面顶角裁圆，底角接文字区是方的）
                    .rounded_t(crate::tokens::radius::CARD)
                    .bg(theme.secondary)
                    .child(crate::cover_img::card(
                        cover_url,
                        crate::cover_img::CoverShape::Card,
                    ))
                    .children((downloaded > 0).then(|| {
                        div()
                            .absolute()
                            .top_2()
                            .right_2()
                            .child(Tag::success().child(SharedString::from(downloaded.to_string())))
                    }))
                    .children((episode_count > 0).then(|| {
                        div()
                            .absolute()
                            .bottom_2()
                            .left_2()
                            .rounded(gpui_kit::px(4.0))
                            .bg(crate::tokens::scrim(70))
                            .px_1p5()
                            .py_0p5()
                            .text_xs()
                            .text_color(crate::tokens::on_media(100))
                            .child(SharedString::from(crate::i18n::tf(
                                "detail.allEpisodesBadge",
                                &[("count", &episode_count.to_string())],
                            )))
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .p_3()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(SharedString::from(title.to_string())),
                    )
                    .children((!heat.is_empty()).then(|| {
                        h_flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            // 热度行走业务令牌（Tauri 卡片同款 text-warm）
                            .text_color(crate::tokens::warm(theme.mode))
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Flame)
                                    .small(),
                            )
                            .child(SharedString::from(heat.to_string()))
                    }))
                    .children((!tags.is_empty()).then(|| {
                        h_flex().flex_nowrap().gap_1().overflow_hidden().children(
                            tags.into_iter().map(|tag| {
                                div()
                                    .flex_shrink_0()
                                    .rounded(gpui_kit::px(4.0))
                                    .bg(theme.secondary)
                                    .px_1p5()
                                    .py_0p5()
                                    .text_size(gpui_kit::px(10.0))
                                    .child(SharedString::from(tag))
                            }),
                        )
                    })),
            )
    }
}
