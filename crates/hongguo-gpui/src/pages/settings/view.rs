//! 设置页视图侧：主题切换、整页渲染入口与分隔线。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::div;

use crate::ui_button::button as Button;

impl super::SettingsView {
    /// 切换主题：立即应用 + 落盘 + 通知壳（auto 观察者换判据）。
    pub(crate) fn set_theme(&mut self, pref: crate::ui_prefs::ThemePref, cx: &mut Context<Self>) {
        use crate::ui_prefs::ThemePref;
        self.theme = pref;
        let mut prefs = crate::ui_prefs::load();
        prefs.theme = pref;
        crate::ui_prefs::save(prefs);
        let mode = pref.resolve(cx.window_appearance());
        crate::tokens::apply(mode, cx);
        // 显式档位把窗口外观钉给平台（macOS 窗口边框跟随）；auto 交还系统
        cx.set_window_appearance(match pref {
            ThemePref::Auto => None,
            ThemePref::Light => Some(gpui_kit::WindowAppearance::Light),
            ThemePref::Dark => Some(gpui_kit::WindowAppearance::Dark),
        });
        cx.emit(SettingsEvent::ThemeChanged(pref));
        cx.notify();
    }
}

impl gpui_kit::Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if !self.loaded {
            h_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element()
        } else {
            v_flex()
                .max_w(gpui_kit::px(768.0))
                .mx_auto()
                .w_full()
                .p_6()
                .gap_4()
                .child(self.render_account_card(cx))
                .child(self.render_dir_card(cx))
                .child(self.render_naming_card(cx))
                .child(self.render_proxy_card(cx))
                .child(separator())
                .child(self.render_playback_card(cx))
                .child(self.render_shortcut_card(cx))
                .child(self.render_boss_key_card(cx))
                .child(self.render_storage_card(cx))
                .child(self.render_update_card(cx))
                .child(
                    h_flex().w_full().justify_end().child(
                        Button::new("settings-save")
                            .label(crate::i18n::t("settings.save"))
                            .primary()
                            .loading(self.saving)
                            // loading 态与 disabled 同理：组件库把它算作惰性
                            // （interactive = !(disabled || loading)），光标回箭头。
                            .when(self.saving, |b| b.cursor_default())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save(window, cx);
                            })),
                    ),
                )
                .into_any_element()
        };
        let theme = cx.theme();
        div()
            .id("settings-page")
            .size_full()
            .overflow_y_scroll()
            .bg(theme.background)
            .text_color(theme.foreground)
            // 捕获态键盘归属：焦点锚在根元素上，捕获中接管按键
            .track_focus(&self.keys_focus)
            .key_context("SettingsKeys")
            .on_key_down(
                cx.listener(|this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                    if this.capturing.is_some() {
                        this.on_capture_key(ev, window, cx);
                    }
                }),
            )
            .child(body)
    }
}

/// 卡片之间的分隔线（组件库 `Separator`：颜色跟主题 border）。
fn separator() -> gpui_kit::AnyElement {
    gpui_kit::component::separator::Separator::horizontal()
        .h(gpui_kit::px(1.0))
        .into_any_element()
}
