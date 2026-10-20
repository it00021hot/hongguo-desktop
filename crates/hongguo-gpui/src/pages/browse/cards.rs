//! 找剧页结果卡：浏览流与搜索结果共用的卡片渲染。

use super::*;

impl super::BrowseView {
    /// 一张结果卡（浏览/搜索共用；字段差异在调用方已归一）。
    pub(super) fn render_card(
        &self,
        card: CardView<'_>,
        card_w: f32,
        cover_h: f32,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
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
        let cover = self.covers.get(cover_url).cloned();
        let tags: Vec<String> = tags
            .iter()
            .filter(|t| !t.is_empty())
            .take(3)
            .cloned()
            .collect();
        v_flex()
            .id(ElementId::from(format!("browse-card-{series_id}")))
            // 宽度跟网格列走（Tauri `w-full` + 响应式列数）：固定 196px 在
            // 1280 窗口下比 Tauri 的 220px 窄一截（并排对拍可见）
            .w(gpui_kit::px(card_w))
            .overflow_hidden()
            .rounded(gpui_kit::px(12.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground))
            .on_click({
                let series_id = series_id.to_string();
                cx.listener(move |this, _, _, cx| {
                    this.play_series(series_id.clone(), cx);
                })
            })
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(gpui_kit::px(cover_h))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .bg(theme.secondary)
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div().size_full().into_any_element(),
                    })
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
                            .bg(gpui_kit::rgba(0x000000b3))
                            .px_1p5()
                            .py_0p5()
                            .text_xs()
                            .text_color(gpui_kit::rgb(0xffffffff))
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
            .into_any_element()
    }
}
