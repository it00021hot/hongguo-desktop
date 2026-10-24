//! 找剧页整页渲染：滚动续页哨兵、筛选区、结果网格、加载/失败/空态。

use super::*;

use gpui_kit::base::Button as BaseButton;

// —— 渲染 ——

impl gpui_kit::Render for BrowseView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 封面预热：把本页要显示的封面排上转码（gpui 资产机制去重），成品图
        // 落盘后 gpui 刷新窗口，渲染期就画得出来。
        let mut cover_urls: Vec<String> = self.feed_items.iter().map(|i| i.cover.clone()).collect();
        cover_urls.extend(self.search_items.iter().map(|i| i.cover.clone()));
        crate::cover_img::prewarm_covers(&cover_urls, cx);
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
        // sm/md/lg/2xl）。卡宽不在这里算：交给布局引擎按列分（见
        // `crate::pages::grid`），手算就得把侧栏宽与页面内边距一起硬编码。
        let viewport_w = window.viewport_size().width.as_f32();
        let grid_cols = crate::pages::grid::cols(viewport_w);
        let search_row = self.render_search_row(cx);
        // 筛选区先建（内部有 cx 闭包，不能与后面取的 theme 借用交叠）
        let filter_section = if searching {
            None
        } else {
            let active_filters = self.active_filter_count();
            let filters_collapsed = self.filters_collapsed;
            let theme = cx.theme();
            // 无障碍名称沿用 Tauri 侧的写法：名称是这行的标签「筛选」，
            // 展开/收起由 aria_expanded 表达——不把状态拼进名字里（中文拼
            // 「收起 筛选」生硬），也不新增 i18n key。
            let filter_toggle_label = crate::i18n::t("browse.filters");
            Some(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        // 载体是 gpui-kit 的 base Button 而不是 div：div 不带焦点
                        // 句柄，键盘用户够不到这行开关；base Button 自带焦点句柄
                        // 与 Enter/Space 激活（视觉仍由 styled 链决定）。
                        BaseButton::new("browse-filter-toggle")
                            .items_center()
                            .gap_1p5()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .accessibility_label(filter_toggle_label)
                            .aria_expanded(!filters_collapsed)
                            // base Button 固定 justify_center，这里改回左对齐，和原来的 div 一致。
                            .justify_start()
                            // base Button 强制 line_height(relative(1.))，文字行盒会比默认矮一截；
                            // 钉回窗口默认行高（黄金比例）才与原来的 div 像素一致。
                            .line_height(gpui_kit::phi())
                            // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
                            .focus_visible(|s| {
                                s.border_color(theme.ring)
                                    .shadow(crate::tokens::focus_ring(theme))
                            })
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
        // 虚拟结果网格：按列数分块成行（卡宽由网格槽给，与原 flex_wrap
        // 网格同几何），末位固定 footer 槽（加载更多/到底提示）。可见行才参与
        // 布局/解码/绘制。
        let rows_total = count.div_ceil(grid_cols);
        let theme = cx.theme();
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
        // 结果区：加载中给卡片网格骨架（与真实结果卡同网格同几何，
        // 数据到位不跳版）；空态给居中提示；否则挂虚拟列表
        let results: gpui_kit::AnyElement = if loading {
            crate::pages::skeleton::card_grid(cx, 10, grid_cols)
        } else if empty {
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
            log::debug!("[BrowseGrid] sync rows={rows_total} cols={grid_cols}");
            let entity = cx.entity();
            let state = self.grid_rows.state();
            // Auto sizing 的 list() 不自报内容高，必须从父容器拿确定高度
            gpui_kit::list(state, move |ix, _window, cx| {
                // 每帧物化的行区间：静止时应只有可见几行反复出现
                log::debug!("[BrowseGrid] render ix={ix}");
                entity.update(cx, |view, cx| view.render_grid_row(ix, grid_cols, cx))
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
    /// 虚拟列表条目：按列数分块的结果行（卡宽由网格槽给，与原 flex_wrap
    /// 网格同几何；缺卡的槽补空，末行右侧留空）；末位是 footer 槽（加载更多/
    /// 到底提示）。
    pub(super) fn render_grid_row(
        &self,
        ix: usize,
        grid_cols: usize,
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
                            .when(loading_more, |b| b.cursor_default())
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
                // 行容器带 id：卡片的 id 是 series_id，同一部剧在推荐流里
                // 重复出现时两处卡片路径相同 → 同一个 a11y NodeId → 重复
                // 节点（debug 构建 panic）。行 id 把每行的作用域隔开。
                .id(gpui_kit::ElementId::from(format!("browse-grid-row-{ix}")))
                .pb_4()
                .child(crate::pages::grid::row(
                    (start..end).map(|i| self.render_card_at(i, cx)).collect(),
                    grid_cols,
                ))
                .into_any_element()
        }
    }

    /// 第 `ix` 张结果卡（浏览流/搜索结果的字段差异在此归一）。
    fn render_card_at(&self, ix: usize, cx: &mut Context<Self>) -> gpui_kit::base::Button {
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
                cx,
            )
        }
    }
}
