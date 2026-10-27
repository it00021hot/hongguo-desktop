//! screenshot feature 的评论面板取证入口：comments / mention 两个场景。

use super::*;

#[cfg(feature = "screenshot")]
use std::time::{Duration, Instant};

#[cfg(feature = "screenshot")]
use gpui_kit::prelude::*;

#[cfg(feature = "screenshot")]
use super::super::controls::CONTROLS_HOLD_SECS;

#[cfg(feature = "screenshot")]
impl HomeView {
    /// 取证辅助（screenshot feature 的 HONGGUO_SHOT_PANEL=comments）：
    /// 打开评论面板与表情选择器（无人值守抓网格 UI）。
    pub(crate) fn debug_open_comments(&mut self, cx: &mut Context<Self>) {
        self.comments_panel_open = true;
        self.emoji_picker_open = true;
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        self.load_comments_first_page(cx);
        cx.notify();
    }

    /// 取证辅助（HONGGUO_SHOT_PANEL=comments-gif）：评论面板 + 表情面板
    /// 直达第三方 GIF 档——动图动画的无人值守验收面（连拍比对格子像素）。
    /// 3 秒后自动触发一次「加载更多」（探针看请求/回包，无人点击）。
    pub(crate) fn debug_open_comments_gif(&mut self, cx: &mut Context<Self>) {
        self.debug_open_comments(cx);
        self.select_emoticon_tab(super::emoticon::EmoticonTab::ThirdParty, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(3))
                .await;
            let _ = this.update(cx, |view, cx| {
                log::debug!("[Emoticon] debug: 自动触发加载更多");
                view.load_more_third_party(cx);
            });
        })
        .detach();
    }

    /// 取证辅助（HONGGUO_SHOT_PANEL=mention）：打开评论面板，拉 @提及候选
    /// 并往输入框排入「@」——渲染期 set_value 后弹层即开（无人值守抓
    /// @候选弹层 UI）。
    pub(crate) fn debug_open_mention(&mut self, cx: &mut Context<Self>) {
        self.comments_panel_open = true;
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        self.load_comments_first_page(cx);
        self.load_mention_users(cx);
        self.mention_insert = Some("@".to_string());
        cx.notify();
    }

    /// 取证辅助（HONGGUO_SHOT_PANEL=comments-caret）：开评论面板、收起
    /// 表情选择器并**程序化聚焦**输入框——「点输入框有没有光标」的无人
    /// 值守取证面（光标绘制有四道闸：焦点 / 非 disabled / 闪烁可见 /
    /// 窗口活跃，抓帧能一并验）。
    pub(crate) fn debug_open_comments_caret(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        self.debug_open_comments(cx);
        self.emoji_picker_open = false;
        if let Some(input) = &self.comment_input {
            use gpui_kit::Focusable as _;
            window.focus(&input.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
    }
}
