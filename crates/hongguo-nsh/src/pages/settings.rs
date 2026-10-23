//! 设置页（Tauri 设置页 / GPUI `pages/settings` 账号卡的 NSH 对齐起步）。
//!
//! P2 先落账号域：登录态卡（账号 / 打码手机号 / 退出两段式确认）与登录
//! 入口（壳级登录面板）。外观 / 播放 / 网络 / 快捷键等区块随 P3 迁入，
//! 页面底部给去向说明。

use nsh_core::ui::prelude::*;

use crate::i18n::t;
use crate::pages::common::{PAGE_PAD, pill};

#[component]
pub fn SettingsPage() -> impl IntoElement {
    let message = signal(String::new());
    let confirm_logout = signal(false);
    let logged = crate::account::logged_in();
    let msg_row = message;

    view()
        .flex_1()
        .w_full()
        .h_full()
        .overflow_hidden()
        .flex_col()
        .px(px(PAGE_PAD))
        .pt(px(PAGE_PAD))
        .gap(px(12.0))
        // 账号卡。
        .child(
            view()
                .w_full()
                .flex_col()
                .gap(px(12.0))
                .p(px(16.0))
                .rounded(px(12.0))
                .bg(theme().group_box)
                .border_1()
                .border_color(theme().border)
                .child(
                    view()
                        .w_full()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text(move || t("settings.account"))
                                .text_size(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme().foreground),
                        ),
                )
                // 已登录：账号 + 打码手机号 + 退出（两段式确认）。
                .child(If::new(
                    move || logged.get(),
                    move || {
                        let Some(account) = crate::account::current() else {
                            return view().into_element();
                        };
                        let uid = account.user_id.clone();
                        let masked = crate::account::mask_mobile(&account.mobile);
                        let has_mobile = !masked.is_empty();
                        view()
                            .w_full()
                            .flex_col()
                            .gap(px(10.0))
                            .child(
                                view()
                                    .w_full()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(12.0))
                                    .child(
                                        text(move || uid.clone())
                                            .text_size(px(13.0))
                                            .text_color(theme().muted_foreground),
                                    )
                                    .child(If::new(
                                        move || has_mobile,
                                        move || {
                                            let masked = masked.clone();
                                            text(move || masked.clone())
                                                .text_size(px(13.0))
                                                .text_color(theme().muted_foreground)
                                        },
                                    ))
                                    .child(view().flex_1())
                                    .child(pill(
                                        "settings-logout",
                                        move || {
                                            if confirm_logout.get() {
                                                t("common.confirm")
                                            } else {
                                                t("settings.logout")
                                            }
                                        },
                                        move || theme().background,
                                        move || theme().danger,
                                        move || {
                                            if !confirm_logout.get() {
                                                confirm_logout.set(true);
                                                return;
                                            }
                                            confirm_logout.set(false);
                                            crate::account::do_logout(message);
                                        },
                                    )),
                            )
                            .into_element()
                    },
                ))
                // 未登录：说明 + 登录入口（壳级登录面板）。
                .child(If::new(
                    move || !logged.get(),
                    move || {
                        view()
                            .w_full()
                            .flex_row()
                            .items_center()
                            .gap(px(12.0))
                            .child(
                                text(move || t("settings.loginPrompt"))
                                    .flex_1()
                                    .text_size(px(13.0))
                                    .text_color(theme().muted_foreground),
                            )
                            .child(pill(
                                "settings-login",
                                move || t("settings.loginAction"),
                                move || theme().primary,
                                move || theme().primary_foreground,
                                crate::account::open_login,
                            ))
                            .into_element()
                    },
                )),
        )
        // 其余区块的去向说明（P3 迁入）。
        .child(
            text(move || t("settings.moreInLaterPhase"))
                .text_size(px(12.0))
                .text_color(theme().muted_foreground),
        )
        // 操作结果（退出成功/失败）。
        .child(If::new(
            move || !msg_row.get().is_empty(),
            move || {
                text(move || msg_row.get())
                    .text_size(px(12.0))
                    .text_color(theme().muted_foreground)
            },
        ))
}
