//! 预约页整页渲染：工具栏/tab 胶囊 + 登录/加载/失败/空态/列表 body + 批量条。

use super::*;

impl ReservationView {
    // —— 渲染 ——

    fn render_toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(self.render_tab_pair(cx))
            .child(div().flex_1())
            .child(search_box(&self.search, cx))
            .children(self.logged_in().then(|| {
                Button::new("reservations-manage")
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
                    }))
            }))
            .into_any_element()
    }

    /// 两个 tab 按钮（数量取各自 TabData；加载中显示省略号）。
    fn render_tab_pair(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .items_center()
            .gap_2()
            .children([true, false].map(|online| {
                let data = if online { &self.online } else { &self.offline };
                let label = if online {
                    crate::i18n::t("reservation.tab.online")
                } else {
                    crate::i18n::t("reservation.tab.offline")
                };
                let count = if data.loading {
                    "…".to_string()
                } else {
                    data.count().to_string()
                };
                let active = self.online_tab == online;
                let button = Button::new(format!("reservations-tab-{online}"))
                    .label(format!("{label} {count}"))
                    .small();
                let button = if active {
                    button.primary()
                } else {
                    button.ghost()
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.online_tab = online;
                    this.selected.clear();
                    cx.notify();
                }))
            }))
            .into_any_element()
    }
}

impl gpui_kit::Render for ReservationView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 封面预热：排上转码（gpui 资产机制去重），成品图落盘后 gpui 刷新窗口。
        let cover_urls: Vec<String> = self
            .online
            .items
            .iter()
            .chain(self.offline.items.iter())
            .flatten()
            .map(|i| i.cover.clone())
            .collect();
        crate::cover_img::prewarm(&cover_urls, hongguo_protocol::cover::EDGE_CARD, cx);
        // 滚动哨兵：滚到底自动续页（Tauri reservation-page.tsx 的
        // IntersectionObserver 同语义；按钮留作兜底）。
        let (has_more, loading) = {
            let tab = self.current();
            (tab.has_more, tab.loading_more || tab.loading)
        };
        if self.scroll.should_load(has_more, loading) {
            self.load_more(cx);
        }
        let logged_in = self.logged_in();
        let body = if !logged_in {
            let theme = cx.theme();
            // Tauri ErrorState（Empty py-16）：流内顶部对齐
            v_flex()
                .items_center()
                .py_16()
                .gap_3()
                .child(
                    gpui_kit::component::Icon::new(gpui_kit::assets::IconName::LogIn)
                        .with_size(gpui_kit::px(32.0))
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(crate::i18n::t("reservation.loginRequired")),
                )
                .child(
                    Button::new("reservations-login")
                        .icon(gpui_kit::assets::IconName::LogIn)
                        .label(crate::i18n::t("settings.loginAction"))
                        .small()
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(ReservationEvent::Login);
                        })),
                )
                .into_any_element()
        } else {
            let current = self.current();
            if current.loading && current.items.is_none() {
                h_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(Spinner::new().small())
                    .into_any_element()
            } else if let Some(err) = current.failed.clone() {
                let theme = cx.theme();
                v_flex()
                    .items_center()
                    .py_16()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("reservation.loadFailed")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(err)),
                    )
                    .child(
                        Button::new("reservations-retry")
                            .label(crate::i18n::t("common.retry"))
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    )
                    .into_any_element()
            } else if self.current_items().is_empty() {
                let theme = cx.theme();
                v_flex()
                    .items_center()
                    // Tauri Empty：流内 py-16 顶部对齐
                    .py_16()
                    .gap_2()
                    .child(
                        gpui_kit::component::Icon::new(gpui_kit::assets::IconName::BellRing)
                            .with_size(gpui_kit::px(32.0))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("reservation.empty")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("reservation.emptyHint")),
                    )
                    .into_any_element()
            } else {
                let managing = self.managing;
                let visible = self.visible_items(cx);
                let has_more = self.current().has_more;
                let loading_more = self.current().loading_more;
                let mut column = v_flex()
                    .w_full()
                    .gap_2()
                    .children(visible.iter().map(|item| {
                        let picked = self.selected.contains(&item.series_id);
                        self.render_row(item, managing, picked, cx)
                    }));
                if has_more {
                    // 加载更多（Tauri 哨兵懒翻页的 GPUI 同语义入口；搜索态的
                    // 后台补齐也走同一 load_more，按钮只是手动兜底）
                    column = column.child(
                        h_flex().w_full().justify_center().pt_2().child(
                            Button::new("reservations-more")
                                .label(if loading_more {
                                    crate::i18n::t("common.loading")
                                } else {
                                    crate::i18n::t("follow.loadMore")
                                })
                                .outline()
                                .small()
                                .disabled(loading_more)
                                .on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
                        ),
                    );
                }
                div()
                    .id("reservations-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(self.scroll.handle())
                    .p_4()
                    .child(column)
                    .into_any_element()
            }
        };
        // 首拉加载期不渲染工具栏：加载态整页居中、不受页内控件影响
        //（用户定稿口径；条件与 body 的加载分支一致）
        let page_loading = logged_in && {
            let current = self.current();
            current.loading && current.items.is_none()
        };
        let toolbar = (logged_in && !page_loading).then(|| self.render_toolbar(cx));
        let batch_bar = (logged_in && self.managing).then(|| {
            let total = self.visible_items(cx).len();
            BatchBar::new(
                self.selected.len(),
                total,
                self.deleting,
                crate::i18n::t("reservation.cancel"),
            )
            .on_toggle_all(cx.listener(|this, _, _, cx| {
                let visible = this.visible_items(cx);
                let total = visible.len();
                if total > 0 && this.selected.len() >= total {
                    this.selected.clear();
                } else {
                    for item in visible {
                        this.selected.insert(item.series_id);
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
            .id("reservations-page")
            .children(toolbar.map(|t| div().w_full().px_4().pt_4().child(t)))
            .child(body)
            .children(batch_bar)
    }
}
