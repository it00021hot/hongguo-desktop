//! 老板键（全局快捷键隐藏/显示主窗口）——Tauri `boss_key_cmd.rs` 同语义移植。
//!
//! 值形态与 Tauri 一致：`Settings.boss_key` 存 tauri accelerator 串
//!（`"CommandOrControl+Alt+H"`，空串 = 未启用），解析/展示/冲突检测在
//! [`accel`]（纯函数，集成测试覆盖）。注册走 `win32` 手写的 FFI——Tauri 用
//! tauri-plugin-global-shortcut，gpui 没有全局快捷键面，这是本项目 FFI 面的
//! 第三个能力（另两个：小屏落位、隐身藏显）。
//!
//! 切换语义与 Tauri 一致：可见 → 隐藏（先暂停播放，声音不漏）；隐藏 →
//! 显示 + 聚焦（前端续播）。macOS 侧待补 FFI（与 mini / 隐身的 Apple 缺口
//! 同口径）：那边设置老板键会明确报错，不做「存上了却从不生效」的假象。

pub(crate) mod accel;

use std::sync::Mutex;

use crate::playback_prefs::ShortcutKeys;

/// 当前已注册的组合（换键注销用；与实态对齐——注册失败不留漂移态）。
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

/// 注册（或换绑 / 注销）老板键。空串 = 注销现有键；注册失败返回可读错误，
/// 并尽力把旧键回滚注回（Tauri `rollback_register` 同款）。
///
/// 调用点与 Tauri 同序：**先注册后落库**——注册失败整个保存不提交，避免
/// 「错误返回了、坏键却已入库，重启照样注册不上」。
pub(crate) fn apply(spec: &str) -> Result<(), String> {
    let spec = spec.trim().to_string();
    let mut cur = CURRENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cur.as_deref() == Some(spec.as_str()) {
        return Ok(());
    }
    let old = cur.take();

    #[cfg(target_os = "windows")]
    {
        if old.is_some() {
            crate::win32::unregister_hotkey();
        }
        if spec.is_empty() {
            return Ok(());
        }
        let hk = accel::parse(&spec)?;
        match crate::win32::register_hotkey(hk.mods, hk.vk) {
            Ok(()) => {
                *cur = Some(spec);
                Ok(())
            }
            Err(e) => {
                // 回滚旧键：失败不外泄（保存本身已在报错），下次保存成功即自愈
                if let Some(o) = old
                    && let Ok(hk) = accel::parse(&o)
                    && crate::win32::register_hotkey(hk.mods, hk.vk).is_ok()
                {
                    *cur = Some(o);
                }
                Err(e)
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = old;
        if spec.is_empty() {
            return Ok(());
        }
        Err("老板键暂未在本平台实现（全局快捷键 FFI 后补）".to_string())
    }
}

/// 老板键是否被按下过（读后清；壳内轮询任务消费）。
pub(crate) fn take_fired() -> bool {
    #[cfg(target_os = "windows")]
    {
        crate::win32::take_hotkey_fired()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// 老板键 vs 播放器快捷键的冲突（末段裸键 vs 播放器键；Tauri
/// `bossKeyConflicts` 同口径）。返回冲突动作的可读名，空 = 无冲突。
pub(crate) fn conflicts(spec: &str, keys: &ShortcutKeys) -> Vec<&'static str> {
    let core = accel::gpui_key_of(spec);
    if core.is_empty() {
        return Vec::new();
    }
    crate::pages::settings::ShortcutAction::ALL
        .iter()
        .filter(|action| action.get(keys).eq_ignore_ascii_case(&core))
        .map(|action| action.label())
        .collect()
}
