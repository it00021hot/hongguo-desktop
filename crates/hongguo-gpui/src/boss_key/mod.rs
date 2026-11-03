//! 老板键（全局快捷键隐藏/显示主窗口）。值形态：tauri accelerator 串
//! （空串 = 未启用），解析/冲突检测在 [`accel`]；注册能力由
//! [`crate::platform`] 提供（Win32 FFI / Carbon）。切换语义：可见 → 隐藏
//! （先暂停，声音不漏）；隐藏 → 显示 + 聚焦（续播）。

pub(crate) mod accel;

use std::sync::Mutex;

use crate::playback_prefs::ShortcutKeys;

/// 当前已注册的组合（换键注销用；与实态对齐——注册失败不留漂移态）。
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

/// 注册（或换绑 / 注销）老板键。空串 = 注销现有键；注册失败返回可读错误，
/// 并尽力把旧键回滚注回。调用点必须先注册后落库——注册失败整个保存不提交。
pub(crate) fn apply(spec: &str) -> Result<(), String> {
    let spec = spec.trim().to_string();
    let mut cur = CURRENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cur.as_deref() == Some(spec.as_str()) {
        return Ok(());
    }
    let old = cur.take();

    if old.is_some() {
        crate::platform::unregister_hotkey();
    }
    if spec.is_empty() {
        return Ok(());
    }
    let hk = accel::parse(&spec)?;
    match crate::platform::register_hotkey(hk.mods, hk.vk) {
        Ok(()) => {
            *cur = Some(spec);
            Ok(())
        }
        Err(e) => {
            // 回滚旧键：失败不外泄（保存本身已在报错），下次保存成功即自愈
            if let Some(o) = old
                && let Ok(hk) = accel::parse(&o)
                && crate::platform::register_hotkey(hk.mods, hk.vk).is_ok()
            {
                *cur = Some(o);
            }
            Err(e)
        }
    }
}

/// 老板键是否被按下过（读后清；壳内轮询任务消费）。
pub(crate) fn take_fired() -> bool {
    crate::platform::take_hotkey_fired()
}

/// 老板键 vs 播放器快捷键的冲突（末段裸键 vs 播放器键；Tauri
/// `bossKeyConflicts` 同口径）。返回冲突动作的可读名，空 = 无冲突。
pub(crate) fn conflicts(spec: &str, keys: &ShortcutKeys) -> Vec<String> {
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
