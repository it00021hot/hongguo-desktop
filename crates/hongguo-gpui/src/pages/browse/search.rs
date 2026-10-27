//! 找剧页搜索：输入框 locale 刷新、提交搜索、结果翻页、退出搜索与顶部
//! 搜索行渲染。

use super::*;

impl super::BrowseView {
    /// 语言切换：刷新搜索框 placeholder，保留已输入关键词。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keyword.update(cx, |input, cx| {
            input.set_placeholder(crate::i18n::t("search.placeholder"), window, cx);
        });
        cx.notify();
    }

    /// 提交搜索：App 综合 tab 搜索；首页精选不足 18 条时自动续拉一页。
    pub(super) fn submit_search(&mut self, cx: &mut Context<Self>) {
        self.suggest_open = false;
        let value = self.keyword.read(cx).value().trim().to_string();
        if value.is_empty() {
            return;
        }
        self.submitted = value;
        self.suggest_items.clear();
        self.search_items.clear();
        self.search_next_offset = 0;
        self.search_id.clear();
        self.search_has_more = false;
        self.load_search_page(true, cx);
    }

    pub(super) fn load_search_page(&mut self, first_page: bool, cx: &mut Context<Self>) {
        if self.search_loading {
            return;
        }
        self.search_loading = true;
        self.search_failed = false;
        // 首页整替（新搜索）：虚拟列表按整替对账（reset，滚动回顶）
        if first_page {
            self.grid_rows.replace();
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let query = self.submitted.clone();
        let offset = if first_page {
            0
        } else {
            self.search_next_offset
        };
        let search_id = if first_page {
            String::new()
        } else {
            self.search_id.clone()
        };
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::search::search_series(&query, offset, &search_id, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.search_loading = false;
                match result {
                    Ok(page) => {
                        if offset == 0 {
                            view.search_items = page.items.clone();
                        } else {
                            view.search_items.extend(page.items.clone());
                        }
                        view.search_next_offset = page.next_offset;
                        view.search_id = page.search_id.clone();
                        view.search_has_more = page.has_more;
                        // 首页精选不足 18 条且还有下一页：自动续拉（Tauri 同款）
                        if first_page && page.has_more && view.search_items.len() < 18 {
                            view.load_search_page(false, cx);
                        }
                    }
                    Err(e) => {
                        log::warn!("[Browse] 搜索失败: {e}");
                        view.search_failed = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn exit_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submitted.clear();
        self.keyword
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suggest_items.clear();
        // 回浏览流（内容整替）：虚拟列表按整替对账（reset，滚动回顶）
        self.grid_rows.replace();
        cx.notify();
    }

    pub(super) fn render_search_row(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        // 取一份主题副本：下面若干 `.children(… self.xxx(cx))` 与 `cx.listener`
        // 都要独占 `cx`，借着一份 `&Theme` 会撞 E0500（拆分前靠闭包内联顺序
        // 侥幸没报，拆成方法后暴露出来）。
        let theme = cx.theme().clone();
        let searching = !self.submitted.is_empty();
        let total = if searching {
            self.search_items.len()
        } else {
            self.feed_items.len()
        };
        // 联想开合（Tauri showSuggest 同款闸门：非搜索态 + 输入 ≥2 字 + 有词）
        let show_suggest = self.suggest_open
            && !self.suggest_items.is_empty()
            && !searching
            && self.keyword.read(cx).value().trim().chars().count() >= 2;
        v_flex()
            .w_full()
            .gap_1()
            .on_key_down(
                cx.listener(|this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                    this.handle_search_key_down(ev, window, cx);
                }),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w(gpui_kit::px(256.0))
                            .child(Input::new(&self.keyword))
                            .children(show_suggest.then(|| self.render_suggest_dropdown(cx))),
                    )
                    .child(
                        Button::new("browse-submit")
                            .icon(gpui_kit::assets::IconName::Search)
                            .label(crate::i18n::t("search.submit"))
                            .disabled(self.keyword.read(cx).value().trim().is_empty())
                            .when(self.keyword.read(cx).value().trim().is_empty(), |b| {
                                b.cursor_default()
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.submit_search(cx))),
                    )
                    .children(searching.then(|| {
                        Button::new("browse-exit-search")
                            .icon(gpui_kit::assets::IconName::X)
                            .label(crate::i18n::t("search.exit"))
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.exit_search(window, cx);
                            }))
                    }))
                    .children((!searching && total > 0).then(|| {
                        div().text_sm().text_color(theme.muted_foreground).child(
                            SharedString::from(crate::i18n::tf(
                                "browse.totalCount",
                                &[("total", &total.to_string())],
                            )),
                        )
                    })),
            )
            .into_any_element()
    }
}
