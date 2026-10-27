//! 下半区 tab 栏 + 选集网格（Tauri Tabs 三触发器同序同计数；选集格 =
//! 序号 + 第 N 集 + 右侧时长角标，续播集描边高亮）。

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, Role, SharedString, Toggled, div};

use super::{DetailTab, DetailView};

impl DetailView {
    pub(super) fn render_tabs(&self, cx: &Context<Self>) -> Option<gpui_kit::AnyElement> {
        let theme = cx.theme();
        let series = self.series.as_ref()?;
        let episode_total = series.episodes.len() as i64;
        let related_total = self
            .related
            .as_ref()
            .map(|r| r.works.len() as i64 + r.guess.len() as i64)
            .unwrap_or(0);
        let tabs = [
            (
                DetailTab::Episodes,
                crate::i18n::t("detail.tabEpisodes"),
                episode_total,
            ),
            (
                DetailTab::Reviews,
                crate::i18n::t("detail.tabComments"),
                self.reviews_total,
            ),
            (
                DetailTab::Related,
                crate::i18n::t("detail.tabRecommend"),
                related_total,
            ),
        ];
        Some(
            v_flex()
                .mt_8()
                .child(
                    // Tauri `TabsList` 同款分段控件：muted 底槽（h-9、p-[3px]、
                    // rounded-lg）里一枚 active 抬起胶囊（bg-background、
                    // rounded-md）。此前是下划线 tab——并排对拍可见不一致
                    h_flex().child(
                        h_flex()
                            .id("detail-tabs")
                            .role(Role::RadioGroup)
                            .h(gpui_kit::px(36.0))
                            .p(gpui_kit::px(3.0))
                            .items_center()
                            .rounded(gpui_kit::px(10.0))
                            .bg(theme.muted)
                            .children(tabs.map(|(tab, label, count)| {
                                let active = self.tab == tab;
                                let text = if count > 0 {
                                    format!("{label} {count}")
                                } else {
                                    label.to_string()
                                };
                                BaseButton::new(ElementId::from(format!("detail-tab-{:?}", tab)))
                                    .role(Role::RadioButton)
                                    .aria_toggled(if active {
                                        Toggled::True
                                    } else {
                                        Toggled::False
                                    })
                                    .accessibility_label(text.clone())
                                    .h_full()
                                    .px_2()
                                    .py_1()
                                    .items_center()
                                    .rounded(gpui_kit::px(8.0))
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .cursor_pointer()
                                    .when(active, |s| {
                                        s.bg(theme.background).text_color(theme.foreground)
                                    })
                                    .when(!active, |s| {
                                        s.text_color(theme.muted_foreground)
                                            .hover(|h| h.text_color(theme.foreground))
                                    })
                                    // base Button 强制 line_height(relative(1.))，
                                    // 文字行盒会比默认矮一截；钉回默认行高才与原
                                    // div 像素一致。
                                    .line_height(gpui_kit::phi())
                                    .focus_visible(|s| {
                                        s.border_color(theme.ring)
                                            .shadow(crate::tokens::focus_ring(theme))
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        // 统一走 select_tab：相关推荐 tab 要顺带
                                        // 拉预约态（未上线卡的预约钮数据源）
                                        this.select_tab(tab, cx);
                                    }))
                                    .child(text)
                            })),
                    ),
                )
                .child(match self.tab {
                    DetailTab::Episodes => self.render_episode_grid(cx),
                    DetailTab::Reviews => self.render_reviews(cx),
                    DetailTab::Related => self.render_related(cx),
                })
                .into_any_element(),
        )
    }

    /// 选集网格（Tauri 2/3/4/5 列响应式 → GPUI 固定 4 列网格；卡内构成
    /// 同款：序号 mono 居中 + 第 N 集 + 时长角标右对齐，续播集高亮描边）。
    fn render_episode_grid(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let series = self.series.as_ref().expect("tab 内容仅在就绪态渲染");
        if series.episodes.is_empty() {
            return div()
                .py_10()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(crate::i18n::t("series.noEpisodes"))
                .into_any_element();
        }
        let (resume_ep, _) = self.resume_entry();
        div()
            .mt_4()
            .grid()
            .grid_cols(4)
            .gap_3()
            .children(series.episodes.iter().map(|ep| {
                let current = ep.vid_index == resume_ep;
                let episode_label =
                    crate::i18n::tf("player.episodeNo", &[("index", &ep.vid_index.to_string())]);
                // 每格可点 = 播放该集：走 base::Button 承担 role/focus/Enter-Space
                // （Tauri 侧同款 `<button type="button">`）。续播集描边只是「上次
                // 看到的位置」的视觉态、不是选择态，故不加 aria_selected。
                BaseButton::new(ElementId::from(format!("detail-ep-{}", ep.vid_index)))
                    .accessibility_label(episode_label.clone())
                    .flex()
                    .items_center()
                    // base Button 强制 justify_center，这里改回左对齐，和原来的 div 一致。
                    .justify_start()
                    .gap_3()
                    .px_4()
                    .py_2p5()
                    .rounded(gpui_kit::px(8.0))
                    .border_1()
                    .when(current, |s| {
                        s.border_color(theme.foreground)
                            .text_color(theme.foreground)
                    })
                    .when(!current, |s| {
                        s.border_color(theme.secondary)
                            .text_color(theme.muted_foreground)
                    })
                    .hover(|s| s.border_color(theme.foreground))
                    .cursor_pointer()
                    // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一
                    // 截；钉回默认行高才与原 div 像素一致。
                    .line_height(gpui_kit::phi())
                    .focus_visible(|s| {
                        s.border_color(theme.ring)
                            .shadow(crate::tokens::focus_ring(theme))
                    })
                    .on_click({
                        let sid = self.series_id.clone();
                        let vid_index = ep.vid_index;
                        cx.listener(move |_, _, _, cx| {
                            cx.emit(super::DetailEvent::Play {
                                series_id: sid.clone(),
                                vid_index,
                                start_secs: 0.0,
                            });
                        })
                    })
                    .child(
                        div()
                            .w(gpui_kit::px(32.0))
                            .flex_shrink_0()
                            .text_center()
                            .font_family("monospace")
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(format!("{}", ep.vid_index)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(SharedString::from(episode_label)),
                    )
                    .children((ep.duration > 0).then(|| {
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(DetailView::fmt_duration(ep.duration)))
                    }))
            }))
            .into_any_element()
    }
}
