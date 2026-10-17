//! 相关推荐 tab：相关作品·系列 置顶 + 猜你喜欢（plan 接口两格，Tauri
//! RelatedWorks/GuessYouLike 同构）。卡 = 封面 + 角标 + 标题 + 评分/播放；
//! 点卡在本页换载（Tauri 同路由跳转同语义）。

use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, div, img};
use hongguo_api::detail::{RelatedItem, fetch_related_series};

use super::DetailView;
use crate::bridge;

impl DetailView {
    /// 相关推荐卡 → 本页换载。**必带档案快照 prefill**（Tauri
    /// `relatedDetailSearch` / `a21ddf2` 同款）：未上线剧解析分集必然失败，
    /// 缺 prefill 会整页报错；带上就走「即将上线 + 预约」降级视图。
    /// 点击与取证探针共用这一段。
    pub(super) fn open_related(&mut self, item: &RelatedItem, cx: &mut Context<Self>) {
        let prefill = super::DetailPrefill {
            title: item.title.clone(),
            cover: item.cover.clone(),
            tags: if item.tag.is_empty() {
                Vec::new()
            } else {
                vec![item.tag.clone()]
            },
            desc: item.video_desc.clone(),
            reserved: false,
        };
        self.series_id.clear();
        self.load_with_prefill(item.series_id.clone(), Some(prefill), cx);
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=related-upcoming`）：
    /// 走同一条 `open_related` 路径打开一条「平台上不存在的未上线剧」——
    /// 验 prefill 兜住「解析失败 → 即将上线降级视图」而不是整页报错。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_upcoming_related(&mut self, cx: &mut Context<Self>) {
        let item = RelatedItem {
            series_id: "9999999999999999999".into(),
            title: "预告新剧（未上线样例）".into(),
            cover: String::new(),
            tag: "即将上线".into(),
            score: 0.0,
            play_cnt: 0,
            episode_cnt: 0,
            video_desc: "平台未上线、无分集可解析：详情页应走即将上线降级视图。".into(),
        };
        self.open_related(&item, cx);
    }

    /// 拉相关推荐（plan 冷响应递增退避在 API 层已做；失败整 tab 留空）。
    pub(super) fn load_related(&mut self, cx: &mut Context<Self>) {
        let sid = self.series_id.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_related_series(&sid, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| match result {
                Ok(related) => {
                    for item in related.works.iter().chain(related.guess.iter()) {
                        view.load_image(item.cover.clone(), cx);
                    }
                    view.related = Some(Arc::new(related));
                    cx.notify();
                }
                Err(e) => log::warn!("[Detail] 相关推荐拉取失败: {e}"),
            });
        })
        .detach();
    }

    pub(super) fn render_related(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let Some(related) = &self.related else {
            return div()
                .mt_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("加载中…")
                .into_any_element();
        };
        v_flex()
            .mt_4()
            .gap_6()
            .children((!related.works.is_empty()).then(|| {
                v_flex()
                    .gap_3()
                    .child(Self::section_title("相关作品·系列"))
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .children(related.works.iter().map(|item| self.related_card(item, cx))),
                    )
            }))
            .children((!related.guess.is_empty()).then(|| {
                v_flex()
                    .gap_3()
                    .child(Self::section_title("猜你喜欢"))
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .children(related.guess.iter().map(|item| self.related_card(item, cx))),
                    )
            }))
            .children(
                (related.works.is_empty() && related.guess.is_empty()).then(|| {
                    div()
                        .py_6()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("暂无相关推荐")
                }),
            )
            .into_any_element()
    }

    fn section_title(text: &str) -> gpui_kit::AnyElement {
        div()
            .text_base()
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .child(SharedString::from(text.to_string()))
            .into_any_element()
    }

    /// 相关卡（related-card.tsx 同位：封面 3:4 + 左上题材角标 + 右下评分角标
    /// （星色）+ 标题 + 集数/播放行）。
    fn related_card(&self, item: &RelatedItem, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self.images.get(&item.cover).cloned();
        let mut stats: Vec<String> = Vec::new();
        if item.episode_cnt > 0 {
            stats.push(format!("全{}集", item.episode_cnt));
        } else if item.tag == "即将上线" {
            stats.push("即将上线".to_string());
        }
        if item.play_cnt > 0 {
            stats.push(format!("{}次播放", Self::fmt_count(item.play_cnt)));
        }
        div()
            .id(ElementId::from(format!(
                "detail-related-{}",
                item.series_id
            )))
            .w(gpui_kit::px(116.0))
            .cursor_pointer()
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| this.open_related(&item, cx))
            })
            .child(
                div()
                    .relative()
                    .h(gpui_kit::px(154.0))
                    .w_full()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(8.0))
                    .bg(theme.secondary)
                    .children(cover.map(|bitmap| {
                        img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                    }))
                    .children((!item.tag.is_empty()).then(|| {
                        div()
                            .absolute()
                            .top_1()
                            .left_1()
                            .px_1p5()
                            .rounded(gpui_kit::px(4.0))
                            .text_xs()
                            .bg(gpui_kit::rgba(0x000000aa))
                            .text_color(gpui_kit::rgb(0xffffff))
                            .child(SharedString::from(item.tag.clone()))
                    }))
                    .children((item.score > 0.0).then(|| {
                        div()
                            .absolute()
                            .right_1()
                            .bottom_1()
                            .px_1()
                            .py_0p5()
                            .rounded(gpui_kit::px(4.0))
                            .bg(gpui_kit::rgba(0x00000099))
                            .text_size(gpui_kit::px(10.0))
                            // 评分角标走 star（Tauri related-card text-star 同款）
                            .text_color(crate::tokens::star(theme.mode))
                            .child(SharedString::from(format!("{:.1}分", item.score)))
                    })),
            )
            .child(
                div()
                    .mt_1p5()
                    .text_sm()
                    .line_height(gpui_kit::relative(1.3))
                    .truncate()
                    .child(SharedString::from(item.title.clone())),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(stats.join(" · "))),
            )
            .into_any_element()
    }
}
