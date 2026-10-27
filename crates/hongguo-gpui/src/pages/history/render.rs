//! 历史页渲染：筛选工具栏 + 加载/失败/空态/列表 body + 未登录提示 + 批量条。

use super::*;

impl HistoryView {
    // —— 渲染 ——

    fn render_toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .children(HistoryTab::ALL.map(|tab| {
                let active = self.tab == tab;
                let button = Button::new(format!("history-tab-{}", tab.label()))
                    .label(tab.label())
                    .small();
                let button = if active {
                    // Tauri ToggleGroupItem：选中 bg-primary text-primary-foreground
                    //（深色主题白底胶囊）；secondary 是深灰胶囊，对比度与真机不同
                    button.primary()
                } else {
                    button.ghost()
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.tab = tab;
                    // 虚拟列表滚动归零（Tauri 换 tab 重挂载同语义）
                    this.scroll.reset();
                    cx.notify();
                }))
            }))
            .child(div().flex_1())
            .child(search_box(&self.search, cx))
            .child(
                Button::new("history-manage")
                    .label(if self.managing {
                        crate::i18n::t("batch.exitManage")
                    } else {
                        crate::i18n::t("batch.manage")
                    })
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.managing = !this.managing;
                        if !this.managing {
                            this.selected.clear();
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

impl gpui_kit::Render for HistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 封面预热：排上转码（gpui 资产机制去重），成品图落盘后 gpui 刷新窗口。
        let cover_urls: Vec<String> = self.items.iter().map(|i| i.cover.clone()).collect();
        crate::cover_img::prewarm_covers(&cover_urls, cx);
        let body = match &self.phase {
            // 首拉加载：骨架铺在列表容器里（与真实列表同 padding，
            // 数据到位不跳版）。行高按 GPUI 真实行算，不照抄 Tauri 的 h-28：
            // 封面 96 + 行内上下 p_3 24 + 行间距（条目槽 pb_2）8 = 128。
            Phase::Loading => div()
                .id("history-skeleton")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .p_4()
                .child(crate::pages::skeleton::rows(cx, 6, 120.0, 8.0))
                .into_any_element(),
            Phase::Failed(msg) => {
                let detail = msg.clone();
                crate::pages::states::error_state(
                    cx,
                    None,
                    crate::i18n::t("history.loadFailed"),
                    Some(detail),
                    Some((
                        "history-retry",
                        crate::pages::states::retry_on(cx.entity(), |this, cx| this.load(cx)),
                    )),
                )
            }
            Phase::Loaded if self.items.is_empty() => crate::pages::states::empty_state(
                cx,
                Some(gpui_kit::assets::IconName::ClockArrowDown),
                crate::i18n::t("history.empty"),
                Some(crate::i18n::t("history.emptyHint")),
            ),
            Phase::Loaded => {
                let shown = self.visible_indices(cx).len();
                if shown == 0 {
                    // 列表非空但被 tab/搜索滤空：给一行轻提示（区分场景）
                    let theme = cx.theme();
                    let query_empty = self.search.read(cx).value().trim().is_empty();
                    let hint = if !query_empty {
                        crate::i18n::t("common.searchNoMatch")
                    } else if self.tab == HistoryTab::Finished {
                        crate::i18n::t("history.noMatchFinished")
                    } else {
                        crate::i18n::t("history.noMatchUnfinished")
                    };
                    v_flex()
                        .items_center()
                        .py_16()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(hint),
                        )
                        .into_any_element()
                } else {
                    let managing = self.managing;
                    let entity = cx.entity();
                    let list_scroll = self.list_scroll.clone();
                    // 虚拟列表（uniform_list）：只有可见行参与布局/解码/绘制，
                    // 滚多深每帧成本都恒定——原来的 visible_count 渐进分批
                    // 与「加载更多」按钮因此退场（分批是 WebView DOM 的补丁，
                    // 这里不需要）。
                    div()
                        .id("history-list")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .p_4()
                        .child(
                            gpui_kit::uniform_list(
                                "history-rows",
                                shown,
                                move |range, _window, cx| {
                                    entity.update(cx, |view, cx| {
                                        view.render_row_range(range, managing, cx)
                                    })
                                },
                            )
                            // Auto sizing 不自报内容高，必须从父容器拿确定高度
                            .flex_1()
                            .track_scroll(&list_scroll),
                        )
                        .into_any_element()
                }
            }
        };
        let toolbar = (matches!(self.phase, Phase::Loaded) && !self.items.is_empty())
            .then(|| self.render_toolbar(cx));
        // 未登录提示行（issue #10）：本地模式可记可管、数据不上云
        let local_hint = (!self.logged_in() && !self.items.is_empty()).then(|| {
            let theme = cx.theme();
            div()
                .w_full()
                .px_4()
                .pt_3()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(crate::i18n::t("history.localModeHint"))
        });
        let batch_bar = self.managing.then(|| {
            let total = self.visible_indices(cx).len();
            BatchBar::new(
                self.selected.len(),
                total,
                self.deleting,
                crate::i18n::t("batch.deleteSelected"),
            )
            .on_toggle_all(cx.listener(|this, _, _, cx| {
                let visible = this.visible_indices(cx);
                let total = visible.len();
                if total > 0 && this.selected.len() >= total {
                    this.selected.clear();
                } else {
                    for index in visible {
                        let series_id = this.items[index].series_id.clone();
                        this.selected.insert(series_id);
                    }
                }
                cx.notify();
            }))
            .on_delete(cx.listener(|this, _, window, cx| {
                this.confirm_batch_delete(window, cx);
            }))
            .into_any_element()
        });
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("history-page")
            .children(local_hint)
            .children(toolbar.map(|t| div().w_full().px_4().pt_4().child(t)))
            .child(body)
            .children(batch_bar)
    }
}
