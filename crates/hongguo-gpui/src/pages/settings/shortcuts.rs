//! 快捷键设置：播放器动作键的录制/冲突拦截/落盘，以及老板键（全局
//! 组合键）卡片与冲突展示。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::div;

use crate::pages::card;

/// GPUI keystroke 键名 → 可读标签（Tauri shortcutKeyLabel 同款）。
fn shortcut_key_label(key: &str) -> String {
    match key {
        "space" => crate::i18n::t("settings.keySpace"),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "escape" => "Esc".to_string(),
        other if other.chars().count() == 1 => other.to_uppercase(),
        other => other.to_string(),
    }
}

impl super::SettingsView {
    /// 捕获态按键处理（仅捕获中生效；Esc 取消，纯修饰键忽略，
    /// 重复键当场拦截——Tauri ShortcutCard 同口径）。
    pub(super) fn on_capture_key(
        &mut self,
        ev: &gpui_kit::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.capturing else {
            return;
        };
        let key = ev.keystroke.key.to_lowercase();
        if key == "escape" {
            self.capturing = None;
            cx.notify();
            return;
        }
        if matches!(
            key.as_str(),
            "shift" | "control" | "alt" | "meta" | "platform" | "fn" | "capslock"
        ) {
            return;
        }
        // 老板键：全局组合必须带 Ctrl/Cmd 或 Alt（Tauri acceleratorFromEvent
        // 同口径），值写进设置草稿（保存时先注册后落库）。
        if target == CaptureTarget::BossKey {
            let mods = &ev.keystroke.modifiers;
            let Some(spec) = crate::boss_key::accel::accelerator_from_keystroke(
                &key,
                mods.control || mods.platform,
                mods.alt,
                mods.shift,
            ) else {
                window.push_notification(
                    Notification::error(crate::i18n::t("settings.bossKeyUnsupported")),
                    cx,
                );
                return; // 保持捕获态，换一个再试
            };
            self.patch(cx, |s| s.boss_key = spec);
            self.capturing = None;
            cx.notify();
            return;
        }
        let CaptureTarget::Shortcut(action) = target else {
            return;
        };
        if let Some(owner) = ShortcutAction::ALL
            .iter()
            .find(|a| **a != action && a.get(&self.shortcut_keys) == key)
        {
            window.push_notification(
                Notification::error(crate::i18n::tf(
                    "settings.keyConflict",
                    &[("key", shortcut_key_label(&key).as_str())],
                )),
                cx,
            );
            // 冲突时保留捕获态（换一个键再试），与 Tauri 的「当场拦截」同效
            let _ = owner;
            cx.notify();
            return;
        }
        action.set(&mut self.shortcut_keys, key);
        self.capturing = None;
        self.persist_shortcut_keys(cx);
        cx.emit(SettingsEvent::ShortcutKeys(self.shortcut_keys.clone()));
        cx.notify();
    }

    /// 快捷键落盘（读-改-写：保留播放偏好其余字段不被覆盖）。
    fn persist_shortcut_keys(&self, cx: &Context<Self>) {
        let keys = self.shortcut_keys.clone();
        crate::bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                let path = hongguo_store::paths::data_dir().join(crate::playback_prefs::FILE_NAME);
                let mut prefs = crate::playback_prefs::load_from(&path);
                prefs.keys = keys;
                crate::playback_prefs::save_to(&path, &prefs);
            })
            .await
            .expect("阻塞池 join")
        });
        let _ = cx;
    }

    /// 老板键卡（Tauri `BossKeyCard` 同款）：全局快捷键隐藏/恢复整个窗口。
    /// 值进设置草稿，保存时先注册后落库；冲突（末段裸键撞播放器键）当场提示
    /// 并在保存时拦截。Tauri 侧也没有「清除」入口，这里不另造。
    pub(super) fn render_boss_key_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let boss = self.current().boss_key;
        let capturing = self.capturing == Some(CaptureTarget::BossKey);
        let display = if boss.trim().is_empty() {
            crate::i18n::t("settings.bossKeyOff")
        } else {
            crate::boss_key::accel::display(&boss)
        };
        let conflicts = crate::boss_key::conflicts(&boss, &self.shortcut_keys);
        let button = Button::new("settings-boss-key")
            .label(if capturing {
                crate::i18n::t("settings.bossKeyCapturing")
            } else {
                display.clone()
            })
            .outline()
            .small();
        let button = if capturing {
            button.text_color(theme.primary)
        } else {
            button
        };
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(crate::i18n::t("settings.bossKeyCardTitle")),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().child(crate::i18n::t("settings.bossKey")))
                    .child(button.on_click(cx.listener(|this, _, window, cx| {
                        this.capturing = Some(CaptureTarget::BossKey);
                        window.focus(&this.keys_focus, cx);
                        cx.notify();
                    }))),
            )
            .children((!conflicts.is_empty()).then(|| {
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(SharedString::from(crate::i18n::tf(
                        "settings.bossKeyConflict",
                        &[("actions", conflicts.join("、").as_str())],
                    )))
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(crate::i18n::t("settings.bossKeyDesc")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(crate::i18n::t("settings.bossKeyGlobalHint")),
            )
            .into_any_element()
    }

    pub(super) fn render_shortcut_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        card(cx)
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(crate::i18n::t("settings.shortcuts")),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("settings-keys-reset")
                            .label(crate::i18n::t("settings.resetKeys"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shortcut_keys = crate::playback_prefs::ShortcutKeys::default();
                                this.capturing = None;
                                this.persist_shortcut_keys(cx);
                                cx.emit(SettingsEvent::ShortcutKeys(this.shortcut_keys.clone()));
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(crate::i18n::t("settings.shortcutsHint")),
            )
            .children(ShortcutAction::ALL.map(|action| {
                let capturing = self.capturing == Some(CaptureTarget::Shortcut(action));
                let label = if capturing {
                    crate::i18n::t("settings.keyCapturing")
                } else {
                    shortcut_key_label(action.get(&self.shortcut_keys))
                };
                let button = Button::new(format!("settings-key-{}", action.label()))
                    .label(label)
                    .outline()
                    .small();
                let button = if capturing {
                    button.text_color(theme.primary)
                } else {
                    button
                };
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .py_1()
                    .child(div().text_sm().child(action.label()))
                    .child(button.on_click(cx.listener(move |this, _, window, cx| {
                        this.capturing = Some(CaptureTarget::Shortcut(action));
                        window.focus(&this.keys_focus, cx);
                        cx.notify();
                    })))
            }))
            .into_any_element()
    }
}

impl super::SettingsView {
    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=boss-set`，配
    /// `HONGGUO_BOSS_KEY=<accelerator>`）：走**真实保存路径**写老板键
    /// （先注册后落库 + 冲突拦截都在 save 里），保存结果只落日志。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_boss_key(
        &mut self,
        spec: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.patch(cx, |s| s.boss_key = spec.to_string());
        self.save(window, cx);
        let saved = self.state.settings().boss_key;
        log::info!("[BossProbe] 写入 {spec:?} 后，库里的老板键 = {saved:?}");
    }
}
