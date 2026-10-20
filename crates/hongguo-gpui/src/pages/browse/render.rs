//! 找剧页整页渲染：滚动续页哨兵、筛选区、结果网格、加载/失败/空态。

use super::*;

// —— 渲染 ——

impl gpui_kit::Render for BrowseView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 滚动哨兵：滚到底自动续页（Tauri browse-page.tsx 的 IntersectionObserver
        // 同语义；浏览流与搜索结果共用同一滚动容器，按当前态选加载入口）。
        let searching = !self.submitted.is_empty();
        let (scroll_has_more, scroll_loading) = if searching {
            (self.search_has_more, self.search_loading)
        } else {
            (self.feed_has_more, self.feed_loading)
        };
        if self.scroll.should_load(scroll_has_more, scroll_loading) {
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
        let searching = !self.submitted.is_empty();
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
        let grid: Vec<gpui_kit::AnyElement> = if searching {
            self.search_items
                .iter()
                .map(|item| {
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
                })
                .collect()
        } else {
            self.feed_items
                .iter()
                .map(|item| {
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
                })
                .collect()
        };
        let (searching_state, feed_state) = (searching, !searching);
        let _ = feed_state;
        let has_more = if searching_state {
            self.search_has_more
        } else {
            self.feed_has_more
        };
        let loading_more = if searching_state {
            self.search_loading
        } else {
            self.feed_loading
        };
        // 加载更多按钮先建（on_click 闭包要 &mut cx，不能与 theme 借用交叠）
        let more_button = has_more.then(|| {
            h_flex().w_full().justify_center().py_2().child(
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
        });
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
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("browse-page")
            .overflow_y_scroll()
            .track_scroll(self.scroll.handle())
            .child(
                v_flex()
                    .w_full()
                    .p_6()
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
                    .child(if empty {
                        v_flex()
                            .w_full()
                            .items_center()
                            .py_16()
                            .child(div().text_sm().text_color(theme.muted_foreground).child(
                                if searching {
                                    crate::i18n::t("search.empty")
                                } else {
                                    crate::i18n::t("browse.empty")
                                },
                            ))
                            .into_any_element()
                    } else {
                        h_flex()
                            .w_full()
                            .flex_wrap()
                            .items_start()
                            .gap_4()
                            .children(grid)
                            .into_any_element()
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
            )
            .into_any_element()
    }
}
