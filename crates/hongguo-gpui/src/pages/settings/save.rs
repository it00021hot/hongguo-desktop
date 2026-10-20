//! 设置保存：草稿归一化、老板键冲突拦截与「先注册后落库」，保存成功
//! 后用服务端归一化值回写表单。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use hongguo_common::model::settings::ProxyMode;

use super::storage::cache_dir_display;

impl super::SettingsView {
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let mut next = self.current();
        next.download_dir = self.dir_input.read(cx).value().trim().to_string();
        next.cache_dir = self.cache_dir_input.read(cx).value().trim().to_string();
        if next.proxy.mode == ProxyMode::Manual {
            next.proxy.url = self.proxy_input.read(cx).value().trim().to_string();
        }
        // 老板键冲突拦截（Tauri submit 同款：全局组合在应用内会同时触发
        // 播放器动作，存了就是坏状态）
        let conflicts = crate::boss_key::conflicts(&next.boss_key, &self.shortcut_keys);
        if !conflicts.is_empty() {
            window.push_notification(
                Notification::error(crate::i18n::tf(
                    "settings.bossKeyConflict",
                    &[("actions", conflicts.join("、").as_str())],
                )),
                cx,
            );
            return;
        }
        // 老板键先注册后落库（Tauri save_settings 同序：注册失败整个保存不
        // 提交——先落库的话错误返回了坏键却已入库，重启后照样注册不上）
        if let Err(e) = crate::boss_key::apply(&next.boss_key) {
            log::warn!("[BossKey] {e}");
            let message = e.to_string();
            window.push_notification(
                Notification::error(crate::i18n::tf(
                    "settings.saveFailed",
                    &[("message", message.as_str())],
                )),
                cx,
            );
            return;
        }
        self.saving = true;
        cx.notify();
        match hongguo_service::settings_service::save(&self.state, next) {
            Ok(saved) => {
                window
                    .push_notification(Notification::success(crate::i18n::t("settings.saved")), cx);
                self.draft = None;
                // 归一化后的值回写表单
                let dir = saved.download_dir.clone();
                self.dir_input
                    .update(cx, |input, cx| input.set_value(dir, window, cx));
                let cache_dir = cache_dir_display(&self.default_cache_dir, &saved.cache_dir);
                self.cache_dir_input
                    .update(cx, |input, cx| input.set_value(cache_dir, window, cx));
                let url = saved.proxy.url.clone();
                self.proxy_input
                    .update(cx, |input, cx| input.set_value(url, window, cx));
                let concurrency = saved.max_concurrency as f32;
                self.concurrency
                    .update(cx, |state, cx| state.set_value(concurrency, window, cx));
                let prefetch = saved.prefetch_episodes as f32;
                self.prefetch
                    .update(cx, |state, cx| state.set_value(prefetch, window, cx));
            }
            Err(e) => {
                log::warn!("[Settings] 保存失败: {e}");
                let message = crate::i18n::app_error_text(&e);
                window.push_notification(
                    Notification::error(crate::i18n::tf(
                        "settings.saveFailed",
                        &[("message", message.as_str())],
                    )),
                    cx,
                );
            }
        }
        self.saving = false;
        cx.notify();
    }
}
