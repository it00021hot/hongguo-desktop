//! 小屏/竖屏入口按钮（控制条右侧的窗口形态切换）。

use super::*;

use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;

use crate::home::player_icons;
use crate::ui_button::button as Button;

impl HomeView {
    /// 小屏播放入口（Tauri onOpenMini 同款）。
    pub(super) fn render_mini_entry(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        // 小屏播放（Tauri onOpenMini 同款）：**同一个窗口**缩成
        // 480×270 落工作区右下角，播放零中断
        Button::new("stage-mini")
            .icon(player_icons::mini_window())
            .custom(chrome_variant(cx))
            .cursor_pointer()
            .small()
            .on_click(cx.listener(|this, _, window, cx| {
                this.enter_mini(window, cx);
            }))
            .into_any_element()
    }

    /// 竖屏播放入口（Tauri 控制栏竖屏钮同位）。
    pub(super) fn render_portrait_entry(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        // 竖屏播放（Tauri 控制栏竖屏钮同位）：窗口收窄成按视频
        // 宽高比算宽的竖条，竖屏视频刚好包住画面不留两侧黑边。
        // 竖屏中的控件换成紧凑条竖屏形态，这里只做「进入」入口
        Button::new("stage-portrait")
            .icon(gpui_kit::assets::IconName::RectangleVertical)
            .custom(chrome_variant(cx))
            .cursor_pointer()
            .small()
            .tooltip(crate::i18n::t("player.portrait"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_portrait(window, cx);
            }))
            .into_any_element()
    }
}
