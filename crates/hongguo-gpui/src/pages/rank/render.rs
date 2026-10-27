//! 排行榜整页渲染：加载/失败/空态、子榜列、筛选区与行卡列表；含截图预览种子。

use super::*;

impl super::RankView {
    /// 虚拟列表条目：`ix` 落在行区间给行卡，末位是 footer 槽
    /// （加载更多按钮 / 到底提示，随 has_more 翻面）。
    pub(super) fn render_list_item(
        &self,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if ix < self.items.len() {
            // 行间 8px 间距（Tauri gap-2 同参）：list() 条目直接相邻堆叠，
            // 原 v_flex gap 容器没了，间距改由条目槽位自带
            return div()
                .w_full()
                .pb_2()
                .child(self.render_row(&self.items[ix], cx))
                .into_any_element();
        }
        if self.has_more {
            h_flex()
                .w_full()
                .justify_center()
                .py_2()
                .child(
                    Button::new("rank-more")
                        .label(if self.loading {
                            crate::i18n::t("feed.loadingMore")
                        } else {
                            crate::i18n::t("follow.loadMore")
                        })
                        .ghost()
                        .small()
                        .disabled(self.loading)
                        .when(self.loading, |b| b.cursor_default())
                        .on_click(cx.listener(|this, _, _, cx| this.load_page(true, cx))),
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
    }

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
        for (i, item) in self.items.iter_mut().enumerate() {
            let url = format!("preview://rank/{i}");
            item.cover = url.clone();
            crate::preview::fixtures::put_preview_image(url, crate::preview::fixtures::cover(i));
        }
        cx.notify();
    }
}

impl gpui_kit::Render for RankView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 封面预热：排上转码（gpui 资产机制去重），成品图落盘后 gpui 刷新窗口。
        let cover_urls: Vec<String> = self.items.iter().map(|i| i.cover.clone()).collect();
        crate::cover_img::prewarm_covers(&cover_urls, cx);
        // 滚动哨兵（虚拟列表版）：滚到列表末尾就续页；新页未测高时
        // is_scrolled_to_end 报 None，天然防住「追加后仍在底部」的连环翻页。
        if self.has_more && !self.loading && self.rows.scrolled_to_end() {
            self.load_page(true, cx);
        }
        // 换键时撞上首拉在途而欠下的重拉：在途响应已落定（loading 转假），
        // 这里补发（switch_list 的登记，见 list.rs）。
        if self.needs_reload && !self.loading {
            self.needs_reload = false;
            self.load_page(false, cx);
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
        let (loading, failed, empty, count) = (
            self.loading && self.items.is_empty(),
            self.failed && self.items.is_empty(),
            self.items.is_empty(),
            self.items.len(),
        );
        // 旧列表后台刷新中（keepPreviousData 展示态）：Tauri RefreshShade
        // 同款——降透明度 + 挡掉鼠标（刷新期点旧行会以旧状态起播/预约）。
        // 真没数据的加载走下面的行骨架，不套这层。
        let refreshing = self.loading && !self.items.is_empty();
        // 虚拟列表对账：有行才挂列表，末位固定一个 footer 槽（行数 + 1）
        if !failed && !empty {
            self.rows.sync(count + 1);
        }
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
                                    .relative()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .when(refreshing, |s| s.opacity(0.5))
                                    .children(if loading {
                                        // 首拉加载：行骨架铺在列表区（与真实列表
                                        // 同容器，数据到位不跳版）。行高按 GPUI
                                        // 真实行算，不照抄 Tauri 的 h-24：封面
                                        // 107 + 行内上下 p_3 24 = 131；行间距
                                        // 另按真实条目槽的 pb_2 给 8。
                                        vec![crate::pages::skeleton::rows(cx, 8, 131.0, 8.0)]
                                    } else if failed {
                                        vec![crate::pages::states::error_state(
                                            cx,
                                            None,
                                            crate::i18n::t("rank.loadFailed"),
                                            None,
                                            Some((
                                                "rank-retry",
                                                crate::pages::states::retry_on(
                                                    cx.entity(),
                                                    |this, cx| this.load_page(false, cx),
                                                ),
                                            )),
                                        )]
                                    } else if empty {
                                        vec![crate::pages::states::empty_state(
                                            cx,
                                            None,
                                            crate::i18n::t("rank.empty"),
                                            None,
                                        )]
                                    } else {
                                        let entity = cx.entity();
                                        let state = self.rows.state();
                                        vec![
                                            gpui_kit::list(state, move |ix, _window, cx| {
                                                entity.update(cx, |view, cx| {
                                                    view.render_list_item(ix, cx)
                                                })
                                            })
                                            // Auto sizing 的 list() 不自报内容高，
                                            // 必须从父容器拿确定高度
                                            .flex_1()
                                            .into_any_element(),
                                        ]
                                    })
                                    // 刷新遮罩：挡掉旧列表上的鼠标交互
                                    //（Tauri RefreshShade 的 pointer-events-none）
                                    .children(refreshing.then(|| {
                                        div()
                                            .id("rank-refresh-shade")
                                            .absolute()
                                            .inset_0()
                                            .occlude()
                                    })),
                            )
                            .into_any_element(),
                    ),
            )
            .into_any_element()
    }
}
