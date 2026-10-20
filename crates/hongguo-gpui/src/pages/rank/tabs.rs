//! 排行榜标签页：内容 tab 归一、顶栏 tab、左侧子榜与筛选面板渲染。

use super::*;

/// 榜单 tab 归一（Tauri normalize-tabs.ts 同口径）：两级形态过滤掉无内容
/// 的 celebrity tab；一级（旧身份）形态把榜单折成子榜、平铺选项包成面板行。
pub(super) fn normalize_tabs(tabs: &[RankTab]) -> Vec<RankTab> {
    if tabs.is_empty() {
        return Vec::new();
    }
    if tabs.iter().any(|t| t.id == "all") {
        return tabs
            .iter()
            .filter(|t| t.id != "ranklist_celebrity" && !t.subs.is_empty())
            .cloned()
            .collect();
    }
    vec![RankTab {
        id: "all".into(),
        name: crate::i18n::t("browse.all"),
        subs: tabs
            .iter()
            .map(|tab| hongguo_api::rank::RankSubList {
                id: tab.id.clone(),
                name: tab.name.clone(),
                description: String::new(),
                panel: if tab.subs.is_empty() {
                    Vec::new()
                } else {
                    vec![hongguo_api::rank::RankPanelRow {
                        name: crate::i18n::t("rank.filter.title"),
                        items: tab
                            .subs
                            .iter()
                            .map(|s| hongguo_api::rank::RankPanelItem {
                                id: s.id.clone(),
                                name: s.name.clone(),
                            })
                            .collect(),
                    }]
                },
                // 一级形态（老版本身份）没有月榜 schema
                background: Vec::new(),
            })
            .collect(),
    }]
}

impl super::RankView {
    // —— 渲染 ——

    /// 内容分类 tab（Tauri `TopBarTabs` portal 同位：壳在顶栏渲染它，
    /// 页面主体里不再有第二行）。两级形态才显示（旧一级形态只有一个合成 tab）。
    pub(crate) fn render_top_tabs(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        if !self.raw_tabs.iter().any(|t| t.id == "all") {
            return None;
        }
        let tabs = normalize_tabs(&self.raw_tabs);
        Some(
            h_flex()
                .id("top-rank-tabs")
                .items_center()
                .gap_0p5()
                .children(tabs.into_iter().map(|tab| {
                    let active = self.selected == tab.id;
                    let button = crate::pill_tab::pill_tab(
                        format!("rank-tab-{}", tab.id),
                        tab.name.clone(),
                        active,
                        cx,
                    );
                    let first_sub = tab.subs.first().map(|s| s.id.clone()).unwrap_or_default();
                    let id = tab.id.clone();
                    button.on_click(cx.listener(move |this, _, _, cx| {
                        if this.selected == id {
                            return;
                        }
                        this.switch_to(id.clone(), first_sub.clone(), cx);
                    }))
                }))
                .into_any_element(),
        )
    }

    pub(super) fn current_tab(&self) -> Option<RankTab> {
        normalize_tabs(&self.raw_tabs)
            .into_iter()
            .find(|t| t.id == self.selected)
    }

    pub(super) fn render_sub_nav(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let subs = self.current_tab().map(|t| t.subs).unwrap_or_default();
        v_flex()
            .id("rank-sub-nav")
            .w(gpui_kit::px(144.0))
            .flex_shrink_0()
            .gap_1()
            .overflow_y_scroll()
            .children(subs.into_iter().map(|sub| {
                let active = self.sub == sub.id;
                let id = sub.id.clone();
                div()
                    .id(ElementId::from(format!("rank-sub-{}", sub.id)))
                    .w_full()
                    .rounded(theme.radius)
                    .px_3()
                    .py_2()
                    .text_sm()
                    .cursor_pointer()
                    .when(active, |s| {
                        s.bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                    })
                    .when(!active, |s| {
                        s.text_color(theme.muted_foreground)
                            .hover(|h| h.bg(theme.accent).text_color(theme.accent_foreground))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.sub == id {
                            return;
                        }
                        let selected = this.selected.clone();
                        this.switch_to(selected, id.clone(), cx);
                    }))
                    .child(SharedString::from(sub.name))
            }))
            .into_any_element()
    }

    pub(super) fn render_filter_panel(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let theme = cx.theme();
        let sub = self
            .current_tab()?
            .subs
            .into_iter()
            .find(|s| s.id == self.sub)?;
        if sub.panel.is_empty() {
            return None;
        }
        let value = self.panel.clone();
        let rows: Vec<gpui_kit::AnyElement> = sub
            .panel
            .into_iter()
            .map(|row| {
                let items: Vec<gpui_kit::AnyElement> = row
                    .items
                    .into_iter()
                    .map(|it| {
                        let selected = it.id == value;
                        let id = it.id.clone();
                        div()
                            .id(ElementId::from(format!("rank-panel-{}", it.id)))
                            .rounded_full()
                            .border_1()
                            .when(selected, |s| {
                                s.border_color(theme.primary)
                                    .bg(theme.secondary)
                                    .text_color(theme.primary)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                            })
                            .when(!selected, |s| {
                                s.border_color(theme.border)
                                    .text_color(theme.muted_foreground)
                            })
                            .px_2p5()
                            .py_1()
                            .text_xs()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.secondary))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pick_panel(id.clone(), cx);
                            }))
                            .child(SharedString::from(it.name))
                            .into_any_element()
                    })
                    .collect();
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(row.name)),
                    )
                    .child(h_flex().flex_wrap().gap_1p5().children(items))
                    .into_any_element()
            })
            .collect();
        Some(
            v_flex()
                .w_full()
                .gap_3()
                .rounded(theme.radius + gpui_kit::px(2.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.group_box)
                .p_3()
                .children(rows)
                .children((!value.is_empty()).then(|| {
                    h_flex().w_full().justify_end().child(
                        Button::new("rank-panel-reset")
                            .label(crate::i18n::t("rank.filter.reset"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pick_panel(String::new(), cx);
                            })),
                    )
                }))
                .into_any_element(),
        )
    }
}
