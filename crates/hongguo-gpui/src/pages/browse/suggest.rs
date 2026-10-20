//! 找剧页搜索联想：防抖取词、键盘导航、候选下拉与单行渲染。

use super::*;

impl super::BrowseView {
    /// 刷新联想（≥2 字才发；防抖 300ms，序号防乱序回填）。
    pub(super) fn refresh_suggest(&mut self, cx: &mut Context<Self>) {
        let kw = self.keyword.read(cx).value().trim().to_string();
        self.suggest_seq += 1;
        let seq = self.suggest_seq;
        if kw.chars().count() < 2 || !self.submitted.is_empty() {
            self.suggest_items.clear();
            self.suggest_open = false;
            return;
        }
        let state = Arc::clone(&self.state);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(300))
                .await;
            let ok = this
                .update(cx, |view, _| view.suggest_seq == seq)
                .unwrap_or(false);
            if !ok {
                return; // 期间又敲了字/视图销毁
            }
            let rx = bridge::spawn(async move {
                let env = state.api_env();
                hongguo_api::search::search_suggest(&kw, &env).await
            });
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if view.suggest_seq != seq {
                    return; // 过期结果丢弃
                }
                view.suggest_items = result.unwrap_or_default();
                cx.notify();
            });
        })
        .detach();
    }

    fn pick_suggest(&mut self, item: SuggestItem, window: &mut Window, cx: &mut Context<Self>) {
        self.suggest_items.clear();
        self.suggest_open = false;
        if !item.series_id.is_empty() {
            self.play_series(item.series_id.clone(), cx);
            return;
        }
        self.keyword.update(cx, |input, cx| {
            input.set_value(item.word.clone(), window, cx)
        });
        self.submitted = item.word;
        self.load_search_page(true, cx);
        cx.notify();
    }

    // 联想键盘导航（Tauri browse-page 输入框 onKeyDown 同款）：↑↓ 高亮
    // 循环、Enter 选中高亮条（无联想时提交搜索）、Esc 收起。
    //
    // 挂冒泡相即可——输入框在 "Input" key context 里自带 MoveUp/
    // MoveDown/Enter/Escape 绑定，行为断言（tests/browse_suggest_keys.rs）
    // 证实：绑定派发之后按键仍会冒泡到祖先 listener，且单行输入里
    // ↑↓ 不挪光标；Enter 的 action 只补发 InputEvent::PressEnter
    //（本页不订阅该事件，故不会与这里的选择动作并行）。
    pub(super) fn handle_search_key_down(
        &mut self,
        ev: &gpui_kit::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = ev.keystroke.key.as_str();
        let suggest_live = self.suggest_open && !self.suggest_items.is_empty();
        match key {
            "down" | "up" => {
                if suggest_live {
                    let len = self.suggest_items.len();
                    self.suggest_index = if key == "down" {
                        (self.suggest_index + 1) % len
                    } else {
                        (self.suggest_index + len - 1) % len
                    };
                    cx.stop_propagation();
                    cx.notify();
                }
            }
            "enter" => {
                if suggest_live {
                    let at = self
                        .suggest_index
                        .min(self.suggest_items.len().saturating_sub(1));
                    let item = self
                        .suggest_items
                        .get(at)
                        .cloned()
                        .or_else(|| self.suggest_items.first().cloned());
                    if let Some(item) = item {
                        self.pick_suggest(item, window, cx);
                    }
                } else {
                    // 联想没开：Enter = 提交搜索（Tauri 表单 onSubmit 同款）
                    self.submit_search(cx);
                }
                cx.stop_propagation();
            }
            "escape" if self.suggest_open => {
                self.suggest_open = false;
                cx.stop_propagation();
                cx.notify();
            }
            _ => {}
        }
    }

    // 联想下拉：锚在输入框正下方（Tauri Popover
    // side=bottom align=start sideOffset=6 同款），
    // deferred 提到后续兄弟之上绘制——否则会被下面的
    // 卡片网格压住
    pub(super) fn render_suggest_dropdown(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        gpui_kit::deferred(
            v_flex()
                .id("browse-suggest-list")
                .absolute()
                .top(gpui_kit::relative(1.0))
                .left_0()
                .right_0()
                .mt(gpui_kit::px(6.0))
                .max_h(gpui_kit::px(320.0))
                .overflow_y_scroll()
                .rounded(gpui_kit::px(12.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .p_1p5()
                .children(
                    self.suggest_items
                        .iter()
                        .take(8)
                        .enumerate()
                        .map(|(i, item)| self.render_suggest_row(i, item, cx)),
                ),
        )
        .with_priority(1)
        .into_any_element()
    }

    /// 单条联想行（Tauri `suggest-row.tsx` 同款）：放大镜 + 有封面的行前置
    /// 竖版缩略图（40×54）+ 命中高亮词条 + 摘要行；命中片段上 warm 令牌，
    /// 带剧集的条目词条加粗。
    fn render_suggest_row(
        &self,
        index: usize,
        item: &SuggestItem,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let active = index
            == self
                .suggest_index
                .min(self.suggest_items.len().saturating_sub(1));
        let word: gpui_kit::AnyElement = if item.parts.is_empty() {
            div()
                .truncate()
                .child(SharedString::from(item.word.clone()))
                .into_any_element()
        } else {
            h_flex()
                .overflow_hidden()
                .children(item.parts.iter().map(|part| {
                    div()
                        .truncate()
                        .when(part.hl, |s| s.text_color(crate::tokens::warm(theme.mode)))
                        .child(SharedString::from(part.text.clone()))
                }))
                .into_any_element()
        };
        h_flex()
            .id(ElementId::from(format!("browse-suggest-{}", item.word)))
            .items_center()
            .gap(gpui_kit::px(10.0))
            .rounded(gpui_kit::px(8.0))
            .px(gpui_kit::px(10.0))
            .py(gpui_kit::px(7.0))
            .text_sm()
            .cursor_pointer()
            .when(active, |s| s.bg(theme.accent))
            .hover(|s| s.bg(theme.accent))
            // 悬停即改高亮（Tauri onHover={setSuggestIndex} 同款）
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && this.suggest_index != index {
                    this.suggest_index = index;
                    cx.notify();
                }
            }))
            .on_click({
                let picked = item.clone();
                cx.listener(move |this, _, window, cx| {
                    this.pick_suggest(picked.clone(), window, cx);
                })
            })
            .child(
                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Search)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .w(gpui_kit::px(40.0))
                    .h(gpui_kit::px(54.0))
                    .flex_shrink_0()
                    .rounded(gpui_kit::px(6.0))
                    .overflow_hidden()
                    .bg(theme.muted)
                    .child(crate::cover_img::card(&item.cover)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(gpui_kit::px(0.0))
                    .gap_0p5()
                    .child(
                        div()
                            .when(!item.series_id.is_empty(), |s| {
                                s.font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            })
                            .child(word),
                    )
                    .children((!item.abstract_text.is_empty()).then(|| {
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .truncate()
                            .child(SharedString::from(item.abstract_text.clone()))
                    })),
            )
            .into_any_element()
    }
}
