//! 详情页主渲染树：整页骨架 + `impl Render`，以及头部统计用的格式化辅助。

use super::*;

impl DetailView {
    /// 语言切换：刷新剧评/回复输入框 placeholder，保留草稿。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = &self.review_input {
            input.update(cx, |input, cx| {
                input.set_placeholder(crate::i18n::t("detail.commentPlaceholder"), window, cx);
            });
        }
        if let Some(input) = &self.review_reply_input {
            input.update(cx, |input, cx| {
                input.set_placeholder(crate::i18n::t("detail.replyToComment"), window, cx);
            });
        }
        cx.notify();
    }

    // —— 渲染 ——
    fn render_page(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        if self.loading || self.series.is_none() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(theme.background)
                .text_color(theme.foreground)
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(if self.loading {
                            crate::i18n::t("common.loading")
                        } else if self.load_failed {
                            crate::i18n::t("common.loadFailed")
                        } else {
                            crate::i18n::t("common.empty")
                        }),
                )
                .children(self.load_failed.then(|| {
                    Button::new("detail-retry")
                        .label(crate::i18n::t("common.retry"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let sid = this.series_id.clone();
                            // 强制重走装载：先清态再 load
                            this.series_id.clear();
                            this.load(sid, cx);
                        }))
                }))
                .into_any_element();
        }
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("detail-page")
            .overflow_y_scroll()
            .child(self.render_back_bar(cx))
            .child(
                div()
                    .max_w(gpui_kit::px(1152.0))
                    .mx_auto()
                    .w_full()
                    .px_8()
                    .pb_8()
                    .child(self.render_header(cx))
                    .child(self.render_record_line(cx))
                    .children(self.render_tabs(cx)),
            )
            .into_any_element()
    }

    /// 计数人性化（详情页头部精度）：万/亿带一位小数、整数省略小数点
    /// （Tauri formatCountPrecise 同口径）。
    pub(super) fn fmt_count(n: i64) -> String {
        match crate::i18n::locale() {
            crate::i18n::Locale::ZhCn => {
                if n >= 100_000_000 {
                    return format!("{:.1}亿", n as f64 / 100_000_000.0).replace(".0", "");
                }
                if n >= 10_000 {
                    return format!("{:.1}万", n as f64 / 10_000.0).replace(".0", "");
                }
            }
            crate::i18n::Locale::EnUs => {
                if n >= 1_000_000_000 {
                    return format!("{:.1}B", n as f64 / 1_000_000_000.0).replace(".0", "");
                }
                if n >= 1_000_000 {
                    return format!("{:.1}M", n as f64 / 1_000_000.0).replace(".0", "");
                }
                if n >= 1_000 {
                    return format!("{:.1}K", n as f64 / 1_000.0).replace(".0", "");
                }
            }
        }
        format!("{n}")
    }

    /// 秒 → mm:ss（选集格时长角标；Tauri formatDuration 同形态）。
    pub(super) fn fmt_duration(secs: i64) -> String {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

impl gpui_kit::Render for DetailView {
    fn render(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // 封面与头像是**两档**：封面 320、头像 64——成品图按「URL 哈希 + 档位」
        // 落盘（`sized_cache_path`），档位对不上就取不到文件（创作者头像会整块
        // 空白：混在封面批次里按 320 预热，渲染侧找的是 64）。
        let mut cover_urls: Vec<String> = self
            .series
            .as_ref()
            .map(|s| vec![s.cover.clone()])
            .unwrap_or_default();
        let mut avatar_urls: Vec<String> = Vec::new();
        if let Some(meta) = self.meta.as_ref() {
            avatar_urls.extend(meta.creators.iter().map(|c| c.avatar.clone()));
        }
        if let Some(related) = self.related.as_ref() {
            cover_urls.extend(related.works.iter().map(|i| i.cover.clone()));
            cover_urls.extend(related.guess.iter().map(|i| i.cover.clone()));
        }
        crate::cover_img::prewarm_covers(&cover_urls, cx);
        crate::cover_img::prewarm_avatars(&avatar_urls, cx);
        // 剧评输入框懒建：DetailView::new 拿不到 window（壳在事件回调里
        // cx.new 创建本视图），首个 render 才有——建一次并挂 Enter 提交。
        if self.review_input.is_none() {
            let input = cx.new(|cx| {
                gpui_kit::component::input::TextareaState::new(window, cx)
                    .placeholder(crate::i18n::t("detail.commentPlaceholder"))
                    // Enter 发表、Shift+Enter 换行（Tauri 富文本输入 onEnter 同款）
                    .submit_on_enter(true)
            });
            cx.subscribe(
                &input,
                |this, _, event: &gpui_kit::component::input::InputEvent, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) {
                        // 订阅回调没有 window（登录墙/回执要它），转交 render 执行
                        this.review_submit_pending = true;
                    }
                    cx.notify();
                },
            )
            .detach();
            self.review_input = Some(input);
        }
        // 剧评回复输入框（同款懒建；Enter 提交走 pending 转交 render）
        if self.review_reply_input.is_none() {
            let input = cx.new(|cx| {
                gpui_kit::component::input::InputState::new(window, cx)
                    .placeholder(crate::i18n::t("detail.replyToComment"))
            });
            cx.subscribe(
                &input,
                |this, _, event: &gpui_kit::component::input::InputEvent, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) {
                        this.review_reply_submit_pending = true;
                    }
                    cx.notify();
                },
            )
            .detach();
            self.review_reply_input = Some(input);
        }
        if let Some(input) = &self.review_input {
            if self.review_clear_input {
                self.review_clear_input = false;
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
            if self.review_submit_pending {
                self.review_submit_pending = false;
                self.submit_review(window, cx);
            }
        }
        if let Some(input) = &self.review_reply_input {
            if self.review_reply_clear {
                self.review_reply_clear = false;
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
            if self.review_reply_submit_pending {
                self.review_reply_submit_pending = false;
                self.submit_review_reply(window, cx);
            }
        }
        self.render_page(cx)
    }
}
