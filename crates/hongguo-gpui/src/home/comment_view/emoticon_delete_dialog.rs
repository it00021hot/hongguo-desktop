//! 表情删除确认浮层（官方同款：删除「我的表情」要二次确认）。

use super::*;

use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;

use crate::ui_button::button as Button;

impl HomeView {
    /// 删除确认浮层（点遮罩 = 取消；确认后走官方同款 `collect_type=2`）。
    pub(super) fn render_emoticon_delete_dialog(
        &self,
        cx: &Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        self.emoticon_delete_target.as_ref().map(|_| {
            // 卡片走主题（Tauri ConfirmDialog = shadcn AlertDialog
            // bg-background）：亮色主题下是亮卡片，别在浮层里钉死深底。
            let theme = cx.theme();
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::rgba(0x00000099))
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.emoticon_delete_target = None;
                        cx.notify();
                    }),
                )
                .child(
                    v_flex()
                        .w(gpui_kit::px(272.0))
                        .p_4()
                        .gap_2p5()
                        .rounded(gpui_kit::px(12.0))
                        .bg(theme.background)
                        .on_mouse_down(
                            gpui_kit::MouseButton::Left,
                            cx.listener(|_, _, _, cx| cx.stop_propagation()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(crate::i18n::t("player.emoticon.removeConfirmTitle")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(crate::i18n::t("player.emoticon.removeConfirmDesc")),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .mt_1()
                                .child(
                                    Button::new("emoticon-del-cancel")
                                        .label(crate::i18n::t("common.cancel"))
                                        .custom(super::super::controls::chrome_variant(cx))
                                        .cursor_pointer()
                                        .small()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.emoticon_delete_target = None;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("emoticon-del-confirm")
                                        .label(crate::i18n::t("common.delete"))
                                        .danger()
                                        .small()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let target = this.emoticon_delete_target.take();
                                            if let Some(item) = target {
                                                this.delete_emoticon(&item, cx);
                                            }
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .into_any_element()
        })
    }
}
