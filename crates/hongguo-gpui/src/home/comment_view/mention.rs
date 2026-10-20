//! @提及候选弹层：加载态、不可用提示与候选横排。

use super::*;

use gpui_kit::component::Icon;
use gpui_kit::component::Sizable;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, div};

impl HomeView {
    pub(super) fn render_mention_popup(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let candidates: Vec<gpui_kit::AnyElement> = if self.mention_pending {
            vec![
                h_flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2p5()
                    .child(gpui_kit::component::spinner::Spinner::new().small())
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui_kit::rgba(0xffffff99))
                            .child(crate::i18n::t("player.comments.mentionLoading")),
                    )
                    .into_any_element(),
            ]
        } else if self.mention_users.is_empty() {
            vec![
                div()
                    .px_3()
                    .py_2p5()
                    .text_xs()
                    .text_color(gpui_kit::rgba(0xffffff99))
                    .child(if self.logged_in() {
                        crate::i18n::t("player.comments.mentionUnavailable")
                    } else {
                        crate::i18n::t("player.interact.loginRequired")
                    })
                    .into_any_element(),
            ]
        } else {
            self.mention_users
                .iter()
                .map(|u| {
                    let (uid, name, avatar, bot) = (
                        u.user_id.clone(),
                        u.user_name.clone(),
                        u.avatar.clone(),
                        u.is_ai_bot,
                    );
                    let click_name = name.clone();
                    v_flex()
                        .id(ElementId::from(format!("mention-{uid}")))
                        .w(gpui_kit::px(56.0))
                        .items_center()
                        .gap_1()
                        .rounded(gpui_kit::px(6.0))
                        .p_1()
                        .cursor_pointer()
                        .hover(|s| s.bg(gpui_kit::rgba(0xffffff1a)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // 选中 = 记提及对象 + 排一段待插入
                            // 文本（render 期 set_value，见
                            // home/mod.rs 的延迟写入）
                            this.mention_user = Some((uid.clone(), click_name.clone()));
                            this.mention_insert = Some(format!("@{click_name}\u{FFFC} "));
                            cx.notify();
                        }))
                        .child(
                            div()
                                .relative()
                                .child(self.render_avatar(&avatar, &name, 44.0))
                                .children(bot.then(|| {
                                    // AI 角标：橙底白勾（Tauri 同位）
                                    div()
                                        .absolute()
                                        .right(gpui_kit::px(-2.0))
                                        .bottom(gpui_kit::px(-2.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(gpui_kit::px(16.0))
                                        .rounded_full()
                                        .bg(gpui_kit::rgb(0xf97316))
                                        .child(
                                            Icon::new(gpui_kit::assets::IconName::Check)
                                                .with_size(gpui_kit::px(10.0))
                                                .text_color(gpui_kit::rgb(0xffffff)),
                                        )
                                })),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_0p5()
                                .text_size(gpui_kit::px(11.0))
                                .text_color(gpui_kit::rgba(0xffffffe6))
                                .child(div().truncate().child(SharedString::from(name.clone())))
                                .children(bot.then(|| {
                                    div()
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(gpui_kit::rgb(0xfbbf24))
                                        .child("Ai")
                                })),
                        )
                        .into_any_element()
                })
                .collect()
        };
        h_flex()
            .id("mention-popup")
            .flex_wrap()
            .items_center()
            .gap_2()
            .rounded(gpui_kit::px(8.0))
            .border_1()
            .border_color(gpui_kit::rgba(0xffffff1a))
            .bg(gpui_kit::rgb(0x262626))
            .p_2()
            .children(candidates)
            .into_any_element()
    }
}
