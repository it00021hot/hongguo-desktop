//! 网络代理设置卡：模式选择、手动地址与「测试连接」。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::Select;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::div;
use hongguo_common::model::settings::ProxyMode;

use crate::bridge;
use crate::pages::card;
use crate::ui_button::button as Button;

impl super::SettingsView {
    fn test_proxy(&mut self, cx: &mut Context<Self>) {
        if self.testing_proxy {
            return;
        }
        self.testing_proxy = true;
        self.test_result = None;
        cx.notify();
        let mut cfg = self.current().proxy;
        if cfg.mode == ProxyMode::Manual {
            cfg.url = self.proxy_input.read(cx).value().trim().to_string();
        }
        let rx = bridge::spawn(async move {
            hongguo_service::settings_service::proxy::test_proxy(&cfg).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.testing_proxy = false;
                match result {
                    Ok(r) => {
                        window.push_notification(
                            if r.ok {
                                let elapsed = r.elapsed_ms.to_string();
                                Notification::success(crate::i18n::tf(
                                    "settings.testOk",
                                    &[("ms", elapsed.as_str())],
                                ))
                            } else {
                                Notification::error(crate::i18n::tf(
                                    "settings.testFail",
                                    &[("message", r.message.as_str())],
                                ))
                            },
                            cx,
                        );
                        view.test_result = Some(r);
                    }
                    Err(e) => {
                        log::warn!("[Settings] 代理测试失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "settings.testFail",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_proxy_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let current = self.current();
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(crate::i18n::t("settings.proxy")),
            )
            .child(Select::new(&self.proxy_select).w_full())
            .children(
                (current.proxy.mode == ProxyMode::Manual).then(|| Input::new(&self.proxy_input)),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("settings-test-proxy")
                            .icon(gpui_kit::assets::IconName::Zap)
                            .label(if self.testing_proxy {
                                crate::i18n::t("settings.testing")
                            } else {
                                crate::i18n::t("settings.testProxy")
                            })
                            .secondary()
                            .small()
                            .disabled(self.testing_proxy)
                            .when(self.testing_proxy, |b| b.cursor_default())
                            .on_click(cx.listener(|this, _, _, cx| this.test_proxy(cx))),
                    )
                    .children(self.test_result.as_ref().map(|r| {
                        let text = if r.ok {
                            format!("{} ms", r.elapsed_ms)
                        } else {
                            crate::i18n::t("common.failed")
                        };
                        if r.ok {
                            Tag::success().child(SharedString::from(text))
                        } else {
                            Tag::danger()
                                .child(SharedString::from(text))
                                .text_color(theme.danger)
                        }
                    })),
            )
            .into_any_element()
    }
}
