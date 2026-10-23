//! 账户卡：登录态展示（头像 / 红果号 / 打码手机号）、退出与头像拉取。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::div;
use hongguo_common::model::AccountState;

use crate::pages::card;

impl super::SettingsView {
    pub(super) fn load_avatar(&mut self, url: String, cx: &mut Context<Self>) {
        let rx = crate::cover_img::fetch(url);
        cx.spawn(async move |this, cx| {
            let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if let Ok(bytes) = bytes
                    && let Some(avatar) = crate::cover_img::decode(&bytes)
                {
                    view.account_avatar = Some(avatar);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn logout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = hongguo_service::login_service::flow::logout(&self.state) {
            log::warn!("[Settings] 退出失败: {e}");
            let message = crate::i18n::app_error_text(&e);
            window.push_notification(
                Notification::error(crate::i18n::tf(
                    "settings.logoutFailed",
                    &[("message", message.as_str())],
                )),
                cx,
            );
            return;
        }
        window.push_notification(
            Notification::success(crate::i18n::t("settings.loggedOut")),
            cx,
        );
        self.draft = None;
        cx.notify();
    }

    fn confirm_logout(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title(crate::i18n::t("nav.logoutConfirmTitle"))
                .description(crate::i18n::t("nav.logoutConfirmBody"))
                .ok_text(crate::i18n::t("settings.logout"))
                .cancel_text(crate::i18n::t("common.cancel"))
                // 组件库取消钮默认不显示：不给 show_cancel，确认框就没有
                // 取消键（Tauri 确认框是「取消 + 确认」两键）
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |view, cx| view.logout(window, cx));
                    true
                })
        });
    }

    // —— 渲染 ——

    pub(super) fn render_account_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let account = self.current().account.clone();
        card(cx)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        gpui_kit::component::Icon::new(gpui_kit::assets::IconName::UserRound)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(crate::i18n::t("settings.account")),
                    ),
            )
            .child(match account {
                Some(account) => {
                    let masked = mask_mobile(&account.mobile);
                    let hongguo_id = hongguo_id_of(&account);
                    let display = if account.user_name.is_empty() {
                        account.user_id.clone()
                    } else {
                        account.user_name.clone()
                    };
                    h_flex()
                        .flex_wrap()
                        .items_center()
                        .gap_3()
                        .child(match &self.account_avatar {
                            Some(bitmap) => {
                                gpui_kit::img(gpui_kit::ImageSource::Render(Arc::clone(bitmap)))
                                    .size_8()
                                    .rounded_full()
                                    .into_any_element()
                            }
                            None => div()
                                .size_8()
                                .rounded_full()
                                .bg(theme.secondary)
                                .into_any_element(),
                        })
                        .child(Tag::secondary().child(SharedString::from(display)))
                        .children((!hongguo_id.is_empty()).then(|| {
                            div().text_xs().text_color(theme.muted_foreground).child(
                                SharedString::from(format!(
                                    "{}: {hongguo_id}",
                                    crate::i18n::t("settings.hongguoId")
                                )),
                            )
                        }))
                        .children((!masked.is_empty()).then(|| {
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(masked))
                        }))
                        .child(div().flex_1())
                        .child(
                            Button::new("settings-logout")
                                .icon(gpui_kit::assets::IconName::LogOut)
                                .label(crate::i18n::t("settings.logout"))
                                .outline()
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_logout(window, cx);
                                })),
                        )
                        .into_any_element()
                }
                None => h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("settings.loginPrompt")),
                    )
                    .child(
                        Button::new("settings-login")
                            .icon(gpui_kit::assets::IconName::LogIn)
                            .label(crate::i18n::t("settings.loginAction"))
                            .small()
                            // Tauri 账户卡的登录是默认 primary 实心钮（深色主题下白底）
                            .primary()
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SettingsEvent::Login);
                            })),
                    )
                    .into_any_element(),
            })
            .into_any_element()
    }
}

/// 手机号打码（138****1234；长度不足原样返回）。
fn mask_mobile(mobile: &str) -> String {
    if mobile.chars().count() >= 11 {
        let head: String = mobile.chars().take(3).collect();
        let tail: String = mobile.chars().skip(7).collect();
        format!("{head}****{tail}")
    } else {
        mobile.to_string()
    }
}

/// 从 user_info 原文提取红果号（biz_user_id；缺失返回空串）。
fn hongguo_id_of(account: &AccountState) -> String {
    let raw = account.raw_profile.as_str();
    if raw.is_empty() {
        return String::new();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return String::new();
    };
    value
        .get("data")
        .and_then(|d| d.get("biz_user_id"))
        .map(|id| match id {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}
