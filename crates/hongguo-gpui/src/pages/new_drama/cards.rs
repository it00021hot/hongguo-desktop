use super::*;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, div};

use crate::ui_button::button as Button;

impl NewDramaView {
    pub(super) fn play_series(&self, series_id: String, cx: &mut Context<Self>) {
        let (vid_index, start_secs) = hongguo_service::play_service::resume::resolve_entry(
            &self.state.store,
            &series_id,
            &[],
        )
        .ok()
        .unwrap_or((1, 0.0));
        cx.emit(NewDramaEvent::Play {
            series_id,
            vid_index,
            start_secs,
        });
    }

    pub(super) fn open_detail(
        &self,
        series_id: String,
        prefill: DetailPrefill,
        cx: &mut Context<Self>,
    ) {
        cx.emit(NewDramaEvent::OpenDetail { series_id, prefill });
    }

    /// 推荐卡点击分流（Tauri handleSelect 同口径）。
    fn select_rank_item(&self, item: &RankItem, cx: &mut Context<Self>) {
        if item.upcoming {
            self.open_detail(
                item.series_id.clone(),
                DetailPrefill {
                    title: item.title.clone(),
                    cover: item.cover.clone(),
                    tags: item.tags.clone(),
                    desc: item.description.clone(),
                    reserved: item.reserved,
                },
                cx,
            );
        } else {
            self.play_series(item.series_id.clone(), cx);
        }
    }

    /// 虚拟列表条目：按列数分块的推荐行（卡宽由网格槽给——Tauri grid-cols 同
    /// 口径，末行不满一排时右侧留空；卡片拉伸到行内最高与 CSS grid 一致）。
    /// 末位是 footer 槽。
    pub(super) fn render_rec_row(
        &self,
        ix: usize,
        cols: usize,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let total = self.items.len();
        if ix * cols >= total {
            if self.has_more {
                h_flex()
                    .w_full()
                    .justify_center()
                    .py_2()
                    .child(
                        Button::new("new-more")
                            .label(if self.loading {
                                crate::i18n::t("feed.loadingMore")
                            } else {
                                crate::i18n::t("follow.loadMore")
                            })
                            .ghost()
                            .small()
                            .disabled(self.loading)
                            .when(self.loading, |b| b.cursor_default())
                            .on_click(cx.listener(|this, _, _, cx| this.load_recommend(true, cx))),
                    )
                    .into_any_element()
            } else {
                let theme = cx.theme();
                h_flex()
                    .w_full()
                    .justify_center()
                    .py_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("follow.noMore")),
                    )
                    .into_any_element()
            }
        } else {
            let end = (ix * cols + cols).min(total);
            // 行间 16px 间距（Tauri grid gap-4 同参，list() 条目自 spacing）
            div()
                .w_full()
                // 行容器带 id：卡片的 id 是 series_id，同一部剧在推荐流里
                // 重复出现时两处卡片路径相同 → 同一个 a11y NodeId → 重复
                // 节点（debug 构建 panic）。行 id 把每行的作用域隔开。
                .id(gpui_kit::ElementId::from(format!("new-grid-row-{ix}")))
                .pb_4()
                .child(crate::pages::grid::row(
                    self.items[ix * cols..end]
                        .iter()
                        .map(|item| self.render_recommend_card(item, cx))
                        .collect(),
                    cols,
                ))
                .into_any_element()
        }
    }

    pub(super) fn render_recommend_card(
        &self,
        item: &RankItem,
        cx: &mut Context<Self>,
    ) -> gpui_kit::base::Button {
        let theme = cx.theme();
        let downloaded = self.downloaded.get(&item.series_id).copied().unwrap_or(0);
        let cover = crate::cover_img::card(&item.cover, crate::cover_img::CoverShape::Card);
        let mut tags: Vec<String> = item.tags.clone();
        if let Some(first) = item.sub_title.split('·').next()
            && !first.is_empty()
        {
            tags.push(first.to_string());
        }
        let tags: Vec<String> = tags.into_iter().filter(|t| !t.is_empty()).take(4).collect();
        // 整卡可点 → base::Button 承担 role/focus/Enter-Space（Tauri 侧
        // `article role="button" tabIndex={0}`，同一语义）。库强制 flex row +
        // 居中，这里要复原原 v_flex 的竖排：flex_col + items_stretch（子件撑满
        // 卡宽，标题才会按卡宽折行）+ justify_start（内容从顶上排）。
        BaseButton::new(ElementId::from(format!("new-card-{}", item.series_id)))
            .accessibility_label(item.title.clone())
            .overflow_hidden()
            .rounded(crate::tokens::radius::CARD)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground.opacity(0.3)).shadow_md())
            .flex_col()
            .items_stretch()
            .justify_start()
            // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一截；
            // 钉回默认行高才与原 div 像素一致。
            .line_height(gpui_kit::phi())
            // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
            .focus_visible(|s| {
                s.border_color(theme.ring)
                    .shadow(crate::tokens::focus_ring(theme))
            })
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| {
                    this.select_rank_item(&item, cx);
                })
            })
            .child(
                crate::pages::grid::cover_box()
                    // 封面顶两角跟着卡片圆角（Tauri：卡片 overflow-hidden +
                    // rounded-xl 把封面顶角裁圆，底角接文字区是方的）
                    .rounded_t(crate::tokens::radius::CARD)
                    .bg(theme.secondary)
                    .child(cover)
                    .children((!item.rec_text.is_empty()).then(|| {
                        h_flex()
                            .absolute()
                            .bottom_1()
                            .left_1()
                            .items_center()
                            .gap_0p5()
                            .text_size(gpui_kit::px(10.0))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            // 封面角标热度走 star（Tauri 卡片 text-star 同款）
                            .text_color(crate::tokens::star(theme.mode))
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Flame)
                                    .small(),
                            )
                            .child(SharedString::from(item.rec_text.clone()))
                    }))
                    .children((item.episode_cnt > 0).then(|| {
                        div()
                            .absolute()
                            .right_1()
                            .bottom_1()
                            .rounded(gpui_kit::px(4.0))
                            .bg(crate::tokens::scrim(60))
                            .px_1()
                            .py_0p5()
                            .text_size(gpui_kit::px(10.0))
                            .text_color(crate::tokens::on_media(100))
                            .child(SharedString::from(crate::i18n::tf(
                                "newDrama.episodeTotal",
                                &[("count", &item.episode_cnt.to_string())],
                            )))
                    }))
                    .children((downloaded > 0).then(|| {
                        div().absolute().top_2().right_2().child(status_badge(
                            downloaded.to_string(),
                            theme.success.opacity(0.15),
                            theme.success,
                            theme.radius,
                        ))
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .p_2()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(SharedString::from(item.title.clone())),
                    )
                    .children((!tags.is_empty()).then(|| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(tags.join("·")))
                    }))
                    .children((item.score > 0.0).then(|| {
                        h_flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Star)
                                    .size_3p5()
                                    .text_color(crate::tokens::star(theme.mode)),
                            )
                            .child(SharedString::from(format!(
                                "{:.1}{}",
                                item.score,
                                crate::i18n::t("newDrama.scoreSuffix")
                            )))
                    })),
            )
    }
}

pub(super) fn status_badge(
    label: impl Into<SharedString>,
    background: gpui_kit::Hsla,
    foreground: gpui_kit::Hsla,
    radius: gpui_kit::Pixels,
) -> gpui_kit::AnyElement {
    h_flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(radius)
        .bg(background)
        .border_1()
        .border_color(background)
        .text_color(foreground)
        .px_2()
        .py_0p5()
        .text_xs()
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(label.into())
        .into_any_element()
}
