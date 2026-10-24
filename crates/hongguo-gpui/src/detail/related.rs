//! 相关推荐 tab：相关作品·系列 置顶 + 猜你喜欢（plan 接口两格，Tauri
//! RelatedWorks/GuessYouLike 同构）。卡 = 封面 + 角标 + 标题 + 评分/播放；
//! 点卡在本页换载（Tauri 同路由跳转同语义）。

use std::sync::Arc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::WindowExt;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, div};
use hongguo_api::detail::{RelatedItem, RelatedSeries, fetch_related_series};

use super::DetailView;
use crate::bridge;
use crate::ui_button::button as Button;

/// 「相关作品」段的容器 id。
///
/// **为什么两段都要自己的 id**：gpui 的 a11y NodeId 是「有 id 的祖先链」的
/// 哈希——两段里同一部剧（服务端两个 cell 各自下发、不去重，猜你喜欢完全可能
/// 命中相关作品里的同 IP 剧）用的是同一个卡片 id，若两段容器都不带 id，两张卡
/// 的路径相同 → 同一个 NodeId → 重复节点（debug 构建直接 panic，release 构建
/// 静默丢节点）。容器带上自己的 id 就把两段隔成两个作用域。
const WORKS_SECTION_ID: &str = "detail-related-works";

/// 「猜你喜欢」段的容器 id（与 [`WORKS_SECTION_ID`] 必须是不同的值）。
const GUESS_SECTION_ID: &str = "detail-related-guess";

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
            let _ = this.update_in(cx, |view, window, cx| match result {
                Ok(related) => {
                    view.related = Some(Arc::new(related));
                    cx.notify();
                }
                Err(e) => {
                    log::warn!("[Detail] 相关推荐拉取失败: {e}");
                    // 失败不能停在「加载中…」：Tauri 由 QueryCache 弹错后空表，
                    // GPUI 用空 RelatedSeries 落到同一空态，并给一次 toast。
                    view.related = Some(Arc::new(RelatedSeries {
                        works: Vec::new(),
                        guess: Vec::new(),
                    }));
                    window.push_notification(
                        gpui_kit::component::notification::Notification::error(crate::i18n::tf(
                            "common.failedWith",
                            &[("message", &crate::i18n::app_error_text(&e))],
                        )),
                        cx,
                    );
                    cx.notify();
                }
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
                .child(crate::i18n::t("common.loading"))
                .into_any_element();
        };
        v_flex()
            .mt_4()
            .gap_6()
            .children((!related.works.is_empty()).then(|| {
                v_flex()
                    .id(WORKS_SECTION_ID)
                    .gap_3()
                    .child(Self::section_title(&crate::i18n::t("detail.relatedWorks")))
                    .child(
                        h_flex().flex_wrap().gap_3().children(
                            related
                                .works
                                .iter()
                                .enumerate()
                                .map(|(ix, item)| self.related_card(item, ix, cx)),
                        ),
                    )
            }))
            .children((!related.guess.is_empty()).then(|| {
                v_flex()
                    .id(GUESS_SECTION_ID)
                    .gap_3()
                    .child(Self::section_title(&crate::i18n::t("detail.guessYouLike")))
                    .child(
                        h_flex().flex_wrap().gap_3().children(
                            related
                                .guess
                                .iter()
                                .enumerate()
                                .map(|(ix, item)| self.related_card(item, ix, cx)),
                        ),
                    )
            }))
            .children(
                (related.works.is_empty() && related.guess.is_empty()).then(|| {
                    div()
                        .py_6()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(crate::i18n::t("detail.recommendEmpty"))
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
    ///
    /// 卡片 id 用**段内序号**而不是 series_id：同一部剧在两段里都可能出现
    /// （服务端不去重），而 gpui 的 a11y NodeId 只认「有 id 的祖先链」——
    /// 段容器已把两段隔开，段内再用序号保证唯一（卡片里的预约钮跟着卡的
    /// 路径走，天然不撞）。
    fn related_card(
        &self,
        item: &RelatedItem,
        ix: usize,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = crate::cover_img::card(&item.cover, crate::cover_img::CoverShape::Inline);
        let mut stats: Vec<String> = Vec::new();
        if item.episode_cnt > 0 {
            // Tauri related-card：detail.episodesCount =「共N集」
            stats.push(crate::i18n::tf(
                "detail.episodesCount",
                &[("count", &item.episode_cnt.to_string())],
            ));
        } else if item.tag == "即将上线" {
            stats.push(crate::i18n::t("player.comingSoon"));
        }
        if item.play_cnt > 0 {
            // Tauri：formatPlayCount + detail.plays =「N万播放」
            stats.push(crate::i18n::tf(
                "browse.playCount",
                &[("count", &Self::fmt_count(item.play_cnt))],
            ));
        }
        let is_upcoming = item.episode_cnt == 0 || item.tag == "即将上线";
        let reserve_id = item.series_id.clone();
        let reserved = self.related_reserved.contains(&reserve_id);
        let reserve_pending = self.related_reserve_pending.contains(&reserve_id);
        // 整卡可点 → 用 base::Button 承担 role/focus/Enter-Space（Tauri 侧是
        // `article role="button" tabIndex={0}`，同一语义）。库强制 flex row +
        // 居中，这里要复原原 div 的 block 竖排：flex_col + items_stretch
        // （子件撑满卡宽，标题才会按卡宽折行）+ justify_start（内容从顶上排，
        // Tauri 注释里点名过「button 会把内容垂直居中，整排一高一低」）。
        BaseButton::new(ElementId::from(format!("related-card-{ix}")))
            .accessibility_label(item.title.clone())
            // Tauri 网格 repeat(auto-fill,minmax(128px,1fr)) 在 1280 窗口下
            // 是 7 列 145px；固定 116 会挤成 9 列（并排对拍可见）
            .w(gpui_kit::px(145.0))
            .flex_col()
            .items_stretch()
            .justify_start()
            // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一
            // 截；钉回默认行高才与原 div 像素一致。
            .line_height(gpui_kit::phi())
            .cursor_pointer()
            .focus_visible(|s| s.shadow(crate::tokens::focus_ring(theme)))
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| this.open_related(&item, cx))
            })
            .child(
                div()
                    .relative()
                    .h(gpui_kit::px(193.0))
                    .w_full()
                    .overflow_hidden()
                    .rounded(crate::tokens::radius::COVER)
                    .bg(theme.secondary)
                    .child(cover)
                    .children((!item.tag.is_empty()).then(|| {
                        div()
                            .absolute()
                            .top_1()
                            .left_1()
                            .px_1p5()
                            .rounded(gpui_kit::px(4.0))
                            .text_xs()
                            .bg(crate::tokens::scrim(50))
                            .text_color(crate::tokens::on_media(95))
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
                            .bg(crate::tokens::scrim(60))
                            .text_size(gpui_kit::px(10.0))
                            // 评分角标走 star（Tauri related-card text-star 同款）
                            .text_color(crate::tokens::star(theme.mode))
                            .child(SharedString::from(format!(
                                "{:.1}{}",
                                item.score,
                                crate::i18n::t("detail.scoreUnit")
                            )))
                    })),
            )
            .child(
                div()
                    .mt_1p5()
                    // Tauri：text-xs + line-clamp-2（两行标题）
                    .text_xs()
                    .line_height(gpui_kit::relative(1.3))
                    .h(gpui_kit::px(32.0))
                    .overflow_hidden()
                    .child(SharedString::from(item.title.clone())),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(stats.join(" · "))),
            )
            // 未上线剧的预约钮（Tauri RelatedCard 的 ReserveButton 同款：
            // 乐观翻转；点击不触发整卡跳详情）
            .children(is_upcoming.then(|| {
                Button::new(format!("related-reserve-{reserve_id}"))
                    .icon(if reserved {
                        gpui_kit::assets::IconName::Check
                    } else {
                        gpui_kit::assets::IconName::Bell
                    })
                    .label(if reserved {
                        crate::i18n::t("reservation.reserved")
                    } else {
                        crate::i18n::t("reservation.action")
                    })
                    .outline()
                    .small()
                    .disabled(reserve_pending)
                    .when(reserve_pending, |b| b.cursor_default())
                    .on_click({
                        let reserve_id = reserve_id.clone();
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.toggle_related_reserve(reserve_id.clone(), window, cx);
                        })
                    })
            }))
            .into_any_element()
    }
}
