//! 找剧页整页渲染：滚动续页哨兵、筛选区、结果网格、加载/失败/空态。

use super::*;

// —— 渲染 ——

impl gpui_kit::Render for BrowseView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 封面预热：把本页要显示的封面排上转码（gpui 资产机制去重），成品图
        // 落盘后 gpui 刷新窗口，渲染期就画得出来。
        let mut cover_urls: Vec<String> = self.feed_items.iter().map(|i| i.cover.clone()).collect();
        cover_urls.extend(self.search_items.iter().map(|i| i.cover.clone()));
        crate::cover_img::prewarm(&cover_urls, hongguo_protocol::cover::EDGE_CARD, cx);
        // 滚动哨兵（虚拟列表版）：滚到列表末尾就按当前态续页；新页未测高时
        // is_scrolled_to_end 报 None，天然防住连环翻页。
        let searching = !self.submitted.is_empty();
        let (has_more, loading_more) = if searching {
            (self.search_has_more, self.search_loading)
        } else {
            (self.feed_has_more, self.feed_loading)
        };
        if has_more && !loading_more && self.grid_rows.scrolled_to_end() {
            if searching {
                self.load_search_page(false, cx);
            } else {
                self.load_browse_page(cx);
            }
        }
        // 结果网格列数 = Tauri SeriesCardGrid 的响应式断点（2/3/4/5/8 列，
        // sm/md/lg/2xl）：卡片宽度按列宽铺满，1280 窗口下 5 列 220px 与
        // Tauri 同宽
        let viewport_w = window.viewport_size().width.as_f32();
        let grid_cols = if viewport_w >= 1536.0 {
            8
        } else if viewport_w >= 1024.0 {
            5
        } else if viewport_w >= 768.0 {
            4
        } else if viewport_w >= 640.0 {
            3
        } else {
            2
        };
        let search_row = self.render_search_row(cx);
        // 筛选区先建（内部有 cx 闭包，不能与后面取的 theme 借用交叠）
        let filter_section = if searching {
            None
        } else {
            let active_filters = self.active_filter_count();
            let filters_collapsed = self.filters_collapsed;
            let theme = cx.theme();
            Some(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .id("browse-filter-toggle")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filters_collapsed = !this.filters_collapsed;
                                let mut prefs = crate::ui_prefs::load();
                                prefs.browse_filters_collapsed = this.filters_collapsed;
                                crate::ui_prefs::save(prefs);
                                cx.notify();
                            }))
                            .child(
                                gpui_kit::component::Icon::new(
                                    gpui_kit::assets::IconName::SlidersHorizontal,
                                )
                                .small(),
                            )
                            .child(crate::i18n::t("browse.filters"))
                            .children((active_filters > 0).then(|| {
                                Tag::secondary()
                                    .child(SharedString::from(active_filters.to_string()))
                            }))
                            .child(
                                gpui_kit::component::Icon::new(if filters_collapsed {
                                    gpui_kit::assets::IconName::ChevronDown
                                } else {
                                    gpui_kit::assets::IconName::ChevronUp
                                })
                                .small(),
                            ),
                    )
                    .children(
                        (!filters_collapsed)
                            .then(|| self.render_filter_panel(cx))
                            .flatten(),
                    )
                    .into_any_element(),
            )
        };
        let (loading, failed, empty, count) = if searching {
            (
                self.search_loading && self.search_items.is_empty(),
                self.search_failed,
                self.search_items.is_empty(),
                self.search_items.len(),
            )
        } else {
            (
                self.feed_loading && self.feed_items.is_empty(),
                self.feed_failed,
                self.feed_items.is_empty(),
                self.feed_items.len(),
            )
        };
        // 网格卡宽 = 列宽（Tauri SeriesCardGrid 的卡片铺满格子）；封面按 3:4
        // 固定高。侧栏 56 + 页面左右 p_6 各 24 从视口宽里扣掉
        let content_w = (viewport_w - 56.0 - 48.0).max(240.0);
        // 减 1px 吸收除不尽/滚动条误差，保证一行正好排满 grid_cols 张
        let card_w =
            (((content_w - (grid_cols as f32 - 1.0) * 16.0) / grid_cols as f32) - 1.0).max(120.0);
        let cover_h = card_w * 4.0 / 3.0;
        // 虚拟结果网格：按列数分块成行（卡片宽 = 列宽，与原 flex_wrap 网格
        // 同几何），末位固定 footer 槽（加载更多/到底提示）。可见行才参与
        // 布局/解码/绘制。
        let rows_total = count.div_ceil(grid_cols);
        let theme = cx.theme();
        // 首拉加载：**整页**居中，搜索框/筛选等页内控件加载期不渲染——
        // 加载位置不受页内控件影响（用户定稿口径）。
        if loading {
            return v_flex()
                .size_full()
                .bg(theme.background)
                .text_color(theme.foreground)
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element();
        }
        // 顶部区（搜索框 + 筛选 + 失败提示）：钉在页首——结果网格是虚拟
        // 列表、自己滚（原来整页一起滚，滚深后每帧成本随已加载卡片数涨）。
        let top = v_flex()
            .w_full()
            .px_6()
            .pt_6()
            .gap_4()
            .child(search_row)
            .children(filter_section)
            .children(failed.then(|| {
                div()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(if searching {
                        crate::i18n::t("browse.searchFailed")
                    } else {
                        crate::i18n::t("browse.loadFailed")
                    })
            }))
            .into_any_element();
        // 结果区：空态给居中提示；否则挂虚拟列表
        let results: gpui_kit::AnyElement = if empty {
            v_flex()
                .w_full()
                .items_center()
                .py_16()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(if searching {
                            crate::i18n::t("search.empty")
                        } else {
                            crate::i18n::t("browse.empty")
                        }),
                )
                .into_any_element()
        } else {
            self.grid_rows.sync(rows_total + 1);
            // 滚动/翻页取证（RUST_LOG=debug 才输出）：sync 只在行数或列数
            // 变化时真正动 ListState，这行打的是「当前喂给虚拟列表的形态」。
            log::debug!("[BrowseGrid] sync rows={rows_total} cols={grid_cols} card_w={card_w}");
            let entity = cx.entity();
            let state = self.grid_rows.state();
            // Auto sizing 的 list() 不自报内容高，必须从父容器拿确定高度
            gpui_kit::list(state, move |ix, _window, cx| {
                // 每帧物化的行区间：静止时应只有可见几行反复出现
                log::debug!("[BrowseGrid] render ix={ix}");
                entity.update(cx, |view, cx| {
                    view.render_grid_row(ix, grid_cols, card_w, cover_h, cx)
                })
            })
            .flex_1()
            .into_any_element()
        };
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(top)
            .child(
                // div() 默认 block：列表的 flex_1 要在 flex 容器里才生效
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .px_6()
                    .pt_4()
                    .pb_6()
                    .child(results),
            )
            .into_any_element()
    }
}

impl super::BrowseView {
    /// 虚拟列表条目：按列数分块的结果行（卡片宽 = 列宽，与原 flex_wrap
    /// 网格同几何，行顶对齐）；末位是 footer 槽（加载更多/到底提示）。
    pub(super) fn render_grid_row(
        &self,
        ix: usize,
        grid_cols: usize,
        card_w: f32,
        cover_h: f32,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let total = if self.submitted.is_empty() {
            self.feed_items.len()
        } else {
            self.search_items.len()
        };
        let rows = total.div_ceil(grid_cols);
        if ix >= rows {
            let searching = !self.submitted.is_empty();
            let (has_more, loading_more) = if searching {
                (self.search_has_more, self.search_loading)
            } else {
                (self.feed_has_more, self.feed_loading)
            };
            if has_more {
                h_flex()
                    .w_full()
                    .justify_center()
                    .py_2()
                    .child(
                        Button::new("browse-load-more")
                            .label(if loading_more {
                                crate::i18n::t("feed.loadingMore")
                            } else {
                                crate::i18n::t("follow.loadMore")
                            })
                            .ghost()
                            .small()
                            .disabled(loading_more)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.submitted.is_empty() {
                                    this.load_browse_page(cx);
                                } else {
                                    this.load_search_page(false, cx);
                                }
                            })),
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
            let start = ix * grid_cols;
            let end = (start + grid_cols).min(total);
            // 行间 16px 间距（Tauri flex-wrap gap-4 同参，list() 条目自 spacing）
            div()
                .w_full()
                .pb_4()
                .child(
                    h_flex().w_full().gap_4().items_start().children(
                        (start..end).map(|i| self.render_card_at(i, card_w, cover_h, cx)),
                    ),
                )
                .into_any_element()
        }
    }

    /// 第 `ix` 张结果卡（浏览流/搜索结果的字段差异在此归一）。
    fn render_card_at(
        &self,
        ix: usize,
        card_w: f32,
        cover_h: f32,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if self.submitted.is_empty() {
            let item = &self.feed_items[ix];
            self.render_card(
                CardView {
                    series_id: &item.series_id,
                    title: &item.title,
                    cover_url: &item.cover,
                    episode_count: item.episode_cnt,
                    heat: &item.heat_text,
                    tags: &item.tags,
                },
                card_w,
                cover_h,
                cx,
            )
        } else {
            let item = &self.search_items[ix];
            let tags = if item.tags.is_empty() {
                item.sub_title
                    .split('·')
                    .next()
                    .map(|s| vec![s.to_string()])
                    .unwrap_or_default()
            } else {
                item.tags.clone()
            };
            let heat = if item.heat_text.is_empty() && item.play_cnt > 0 {
                crate::i18n::tf(
                    "browse.playCount",
                    &[("count", &format_play_count(item.play_cnt))],
                )
            } else {
                item.heat_text.clone()
            };
            self.render_card(
                CardView {
                    series_id: &item.series_id,
                    title: &item.title,
                    cover_url: &item.cover,
                    episode_count: item.episode_cnt,
                    heat: &heat,
                    tags: &tags,
                },
                card_w,
                cover_h,
                cx,
            )
        }
    }
}
