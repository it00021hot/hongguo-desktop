use super::*;

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, div};

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

    pub(super) fn render_recommend_card(
        &self,
        item: &RankItem,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let downloaded = self.downloaded.get(&item.series_id).copied().unwrap_or(0);
        let cover = crate::cover_img::card(&item.cover);
        let mut tags: Vec<String> = item.tags.clone();
        if let Some(first) = item.sub_title.split('·').next()
            && !first.is_empty()
        {
            tags.push(first.to_string());
        }
        let tags: Vec<String> = tags.into_iter().filter(|t| !t.is_empty()).take(4).collect();
        v_flex()
            .id(ElementId::from(format!("new-card-{}", item.series_id)))
            .w_full()
            .overflow_hidden()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground.opacity(0.3)).shadow_md())
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| {
                    this.select_rank_item(&item, cx);
                })
            })
            .child(
                div()
                    .relative()
                    .w_full()
                    .aspect_ratio(3.0 / 4.0)
                    .flex_shrink_0()
                    .overflow_hidden()
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
                            .bg(gpui_kit::rgba(0x00000099))
                            .px_1()
                            .py_0p5()
                            .text_size(gpui_kit::px(10.0))
                            .text_color(gpui_kit::rgb(0xffffffff))
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
            .into_any_element()
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
