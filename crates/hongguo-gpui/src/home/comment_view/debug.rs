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
}
