//! 评论/回复删除确认浮层。

use super::*;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::*;

use crate::ui_button::button as Button;

impl HomeView {
    pub(super) fn render_delete_dialog(&self, cx: &Context<Self>) -> Option<gpui_kit::AnyElement> {
        self.comment_delete_target.is_some().then(|| {
            // 删除确认浮层（Tauri ConfirmDialog 同款语义：comment/del
            // service_id=4，评论与回复同款；点遮罩=取消）
            let is_reply = self
                .comment_delete_target
                .as_ref()
                .and_then(|(_, p)| p.as_ref())
                .is_some();
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::rgba(0x00000099))
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.cancel_delete(cx)),
                )
                .child({
                    // 卡片走主题（Tauri ConfirmDialog = shadcn AlertDialog
                    // bg-background）：亮色主题下是亮卡片，别在浮层里钉死
                    // 深底把主题外的用户甩回暗色。
                    let theme = cx.theme();
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
                                .child(if is_reply {
                                    crate::i18n::t("player.comments.deleteReplyTitle")
                                } else {
                                    crate::i18n::t("player.comments.deleteCommentTitle")
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(crate::i18n::t("player.comments.deleteDesc")),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .mt_1()
                                .child(
                                    Button::new("comment-del-cancel")
                                        .label(crate::i18n::t("common.cancel"))
                                        .custom(super::super::controls::chrome_variant(cx))
                                        .cursor_pointer()
                                        .small()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.cancel_delete(cx);
                                        })),
                                )
                                .child(
                                    Button::new("comment-del-confirm")
                                        .label(if self.comment_deleting {
                                            crate::i18n::t("batch.deleting")
                                        } else {
                                            crate::i18n::t("common.delete")
                                        })
                                        .danger()
                                        .small()
                                        .disabled(self.comment_deleting)
                                        .when(self.comment_deleting, |b| b.cursor_default())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.confirm_delete(cx);
                                        })),
                                ),
                        )
                })
                .into_any_element()
        })
    }
}
