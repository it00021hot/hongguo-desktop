//! 排行榜整页渲染：加载/失败/空态、子榜列、筛选区与行卡列表；含截图预览种子。

use super::*;

impl super::RankView {
    /// 预览种子（screenshot feature）：灌假 tabs/子榜/榜单与合成封面。
    /// `rank-empty` = 空态分支；`rank-filter` = 筛选面板展开。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "rank-empty";
        let loading = scene == "rank-loading";
        self.raw_tabs = crate::preview::fixtures::rank_tabs();
        self.selected = "all".into();
        self.sub = "ranklist_hot_sc".into();
        self.filter_open = scene == "rank-filter";
        self.loading = loading;
        self.failed = false;
        self.items = if empty || loading {
            Vec::new()
        } else {
            let mut items = crate::preview::fixtures::rank_items();
            if let Some(last) = items.last_mut() {
                last.upcoming = true;
                last.reserved = true;
            }
            items
        };
        for (i, item) in self.items.iter().enumerate() {
            self.covers
                .insert(item.cover.clone(), crate::preview::fixtures::cover(i));
        }
        cx.notify();
    }
}

impl gpui_kit::Render for RankView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 首拉加载：整页剩余区居中（含子榜导航区域）。居中在右侧列表列里
        // 会读成「偏右」（子榜列占了左侧），用户实测反馈；页面级分支与
        // 创作者主页同款。
        if self.loading && self.items.is_empty() {
            let theme = cx.theme();
            return v_flex()
                .size_full()
                .bg(theme.background)
                .text_color(theme.foreground)
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element();
        }
        // 滚动哨兵：滚到底自动续页（Tauri rank-page.tsx 的 IntersectionObserver
        // 同语义；「加载更多」按钮留作键盘/兜底入口）。
        if self.scroll.should_load(self.has_more, self.loading) {
            self.load_page(true, cx);
        }
        let sub_nav = self.render_sub_nav(cx);
        let filter_panel = self
            .filter_open
            .then(|| self.render_filter_panel(cx))
            .flatten();
        let filter_button = self
            .current_tab()
            .and_then(|t| t.subs.into_iter().find(|s| s.id == self.sub));
        let has_panel = filter_button
            .as_ref()
            .map(|s| !s.panel.is_empty())
            .unwrap_or(false);
        let sub_name = filter_button
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let sub_desc = filter_button
            .as_ref()
            .map(|s| s.description.clone())
            .unwrap_or_default();
        let panel_label = filter_button
            .as_ref()
            .and_then(|s| {
                s.panel
                    .iter()
                    .flat_map(|r| r.items.iter())
                    .find(|it| it.id == self.panel && !it.id.is_empty())
                    .map(|it| it.name.clone())
            })
            .unwrap_or_else(|| crate::i18n::t("rank.filter.all"));
        let filter_open = self.filter_open;
        let filter_toggle = has_panel.then(|| {
            let theme = cx.theme();
            Button::new("rank-filter-toggle")
                .icon(gpui_kit::assets::IconName::SlidersHorizontal)
                .label(panel_label)
                .outline()
                .text_color(if filter_open {
                    theme.primary
                } else {
                    theme.foreground
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.filter_open = !this.filter_open;
                    cx.notify();
                }))
        });
        let (failed, empty, has_more, count) = (
            self.failed && self.items.is_empty(),
            self.items.is_empty(),
            self.has_more,
            self.items.len(),
        );
        let more_button = has_more.then(|| {
            let loading_more = self.loading;
            h_flex().w_full().justify_center().py_2().child(
                Button::new("rank-more")
                    .label(if loading_more {
                        crate::i18n::t("feed.loadingMore")
                    } else {
                        crate::i18n::t("follow.loadMore")
                    })
                    .ghost()
                    .small()
                    .disabled(loading_more)
                    .on_click(cx.listener(|this, _, _, cx| this.load_page(true, cx))),
            )
        });
        let rows: Vec<gpui_kit::AnyElement> = self
            .items
            .iter()
            .map(|item| self.render_row(item, cx))
            .collect();
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("rank-page")
            // Tauri rank-page 根节点无内边距（`flex h-full min-h-0 flex-col
            // gap-4`）：子榜贴侧栏、内容行贴右缘。GPUI 原来 p_4 把整页内缩
            // 16px（并排对拍 sub-nav 67 vs 56、首行 232 vs 219）
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    // 禁 cross 居中：右侧列高于行高时会被整体居中、头部裁出顶
                    // （gpui 默认 items_center 的经典坑）
                    .items_stretch()
                    .gap_4()
                    .child(sub_nav)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            // WebView2 的 6px 细滚动条占内容宽，Tauri 榜单
                            // 行卡右缘因此比视口右缘内收 10px（滚动条 + 间隙）。
                            // GPUI 滚动条是悬浮层，不占宽；显式留出同一 gutter，
                            // 否则行卡与顶栏筛选钮会向右多伸 10px。
                            .pr(gpui_kit::px(10.0))
                            .gap_2()
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .min_w_0()
                                            .items_center()
                                            .gap_2()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .child(SharedString::from(sub_name))
                                            .children((!sub_desc.is_empty()).then(|| {
                                                div()
                                                    .text_xs()
                                                    .truncate()
                                                    .child(SharedString::from(sub_desc))
                                            })),
                                    )
                                    .children(filter_toggle),
                            )
                            .children(filter_panel)
                            .child(
                                div()
                                    .id("rank-list")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .track_scroll(self.scroll.handle())
                                    .child(
                                        v_flex()
                                            .w_full()
                                            .gap_2()
                                            .children(if failed {
                                                vec![
                                                    v_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .py_16()
                                                        .gap_3()
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .text_color(theme.muted_foreground)
                                                                .child(crate::i18n::t(
                                                                    "rank.loadFailed",
                                                                )),
                                                        )
                                                        .child(
                                                            Button::new("rank-retry")
                                                                .label(crate::i18n::t(
                                                                    "common.retry",
                                                                ))
                                                                .on_click(cx.listener(
                                                                    |this, _, _, cx| {
                                                                        this.load_page(false, cx)
                                                                    },
                                                                )),
                                                        )
                                                        .into_any_element(),
                                                ]
                                            } else if empty {
                                                vec![
                                                    v_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .py_16()
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .text_color(theme.muted_foreground)
                                                                .child(crate::i18n::t(
                                                                    "rank.empty",
                                                                )),
                                                        )
                                                        .into_any_element(),
                                                ]
                                            } else {
                                                rows
                                            })
                                            .children(more_button)
                                            .children((!has_more && count > 0).then(|| {
                                                h_flex().w_full().justify_center().py_2().child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(theme.muted_foreground)
                                                        .child(crate::i18n::t("follow.noMore")),
                                                )
                                            })),
                                    ),
                            )
                            .into_any_element(),
                    ),
            )
            .into_any_element()
    }
}
