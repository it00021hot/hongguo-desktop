//! 老板键组合的纯函数（无副作用、无 crate 依赖——集成测试按源文件引入，
//! 见 `tests/boss_key.rs`）。
//!
//! 形态与 Tauri 完全一致：tauri accelerator 串（如 `"CommandOrControl+Alt+H"`，
//! 捕获自键盘物理键、末段是裸键名），空串 = 未启用。注册参数（修饰位 +
//! Win32 虚拟键码）也在这里算，`win32` 只负责把参数交给系统。
//!
//! 注册参数（MOD_* / `parse` / `Hotkey`）两端都编译：Windows 直接交给
//! `RegisterHotKey`，macOS 经 [`carbon_mods`]/[`mac_keycode`] 换成 Carbon
//! 的修饰位与键码（两个映射都是纯函数，集成测试覆盖）。
//!
//! 修饰键位（Win32 `RegisterHotKey` 的 fsModifiers 值；与系统 ABI 对齐）。
pub(crate) const MOD_ALT: u32 = 0x0001;
pub(crate) const MOD_CONTROL: u32 = 0x0002;
pub(crate) const MOD_SHIFT: u32 = 0x0004;
pub(crate) const MOD_WIN: u32 = 0x0008;

/// 组合 → 末段裸键名（小写，未做 gpui 键名归一）。空串/残串返回空。
pub(crate) fn core_key(spec: &str) -> String {
    spec.split('+')
        .map(|p| p.trim().to_lowercase())
        .rfind(|p| !p.is_empty())
        .unwrap_or_default()
}

/// 组合 → 展示形态（`CommandOrControl` → `Ctrl/Cmd`，` + ` 连接）；空串原样。
pub(crate) fn display(spec: &str) -> String {
    let parts: Vec<String> = spec
        .split('+')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| {
            if p.eq_ignore_ascii_case("CommandOrControl") {
                "Ctrl/Cmd".to_string()
            } else {
                p.to_string()
            }
        })
        .collect();
    parts.join(" + ")
}

/// 组合 → gpui keystroke 键名（与播放器快捷键表同一命名空间，冲突检测用）。
pub(crate) fn gpui_key_of(spec: &str) -> String {
    let core = core_key(spec);
    match core.as_str() {
        "space" => "space".to_string(),
        "up" | "arrowup" => "up".to_string(),
        "down" | "arrowdown" => "down".to_string(),
        "left" | "arrowleft" => "left".to_string(),
        "right" | "arrowright" => "right".to_string(),
        "home" | "end" | "pageup" | "pagedown" | "insert" | "delete" | "enter" | "tab"
        | "escape" => core,
        "num0" | "num1" | "num2" | "num3" | "num4" | "num5" | "num6" | "num7" | "num8" | "num9" => {
            core
        }
        other => other.to_string(),
    }
}

/// 注册参数：修饰位（`MOD_*`）+ Win32 虚拟键码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hotkey {
    pub(crate) mods: u32,
    pub(crate) vk: u32,
}

/// accelerator → 注册参数。返回 Err = 缺修饰键 / 键名不认识（设置页据此提示）。
pub(crate) fn parse(spec: &str) -> Result<Hotkey, String> {
    let parts: Vec<String> = spec
        .split('+')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    let Some((core, mods)) = parts.split_last() else {
        return Err(crate::i18n::t("settings.bossKeyEmpty"));
    };
    let mut bits = 0u32;
    for m in mods {
        let m = m.to_lowercase();
        bits |= match m.as_str() {
            "commandorcontrol" | "ctrl" | "control" | "cmd" | "command" | "meta" => MOD_CONTROL,
            "super" | "win" => MOD_WIN,
            "alt" | "option" => MOD_ALT,
            "shift" => MOD_SHIFT,
            other => {
                return Err(crate::i18n::tf(
                    "settings.bossKeyUnknownModifier",
                    &[("key", other)],
                ));
            }
        };
    }
    // 全局快捷键必须带 Ctrl/Cmd 或 Alt（Tauri 侧同口径：防劫持打字）
    if bits & (MOD_CONTROL | MOD_ALT) == 0 {
        return Err(crate::i18n::t("settings.bossKeyNeedsModifier"));
    }
    let vk = vk_of(core)
        .ok_or_else(|| crate::i18n::tf("settings.bossKeyUnknownKey", &[("key", core)]))?;
    Ok(Hotkey {
        mods: bits | MOD_NOREPEAT,
        vk,
    })
}

/// `MOD_NOREPEAT`（0x4000）：按住不连发——老板键是切换语义，连发会把窗口
/// 藏了又显。只被 `parse` 使用（macOS 正式构建不编译）。
const MOD_NOREPEAT: u32 = 0x4000;

/// 裸键名 → Win32 虚拟键码（只收 GPUI 能稳定报出、且 Win32 一一对应的键）。
fn vk_of(core: &str) -> Option<u32> {
    let s = core.to_lowercase();
    // 字母 / 数字（字母给大写 ASCII 即 VK；数字给 '0'..'9'）
    if s.len() == 1 {
        let c = s.as_bytes()[0] as char;
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase() as u32);
        }
        if c.is_ascii_digit() {
            return Some(c as u32);
        }
    }
    // 功能键 F1..F24
    if let Some(n) = s.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x70 + n - 1);
    }
    // 小键盘 Num0..Num9
    if let Some(n) = s.strip_prefix("num").and_then(|n| n.parse::<u32>().ok())
        && n <= 9
    {
        return Some(0x60 + n);
    }
    Some(match s.as_str() {
        "space" => 0x20,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "left" | "arrowleft" => 0x25,
        "up" | "arrowup" => 0x26,
        "right" | "arrowright" => 0x27,
        "down" | "arrowdown" => 0x28,
        "insert" => 0x2D,
        "delete" => 0x2E,
        "enter" => 0x0D,
        "escape" => 0x1B,
        "tab" => 0x09,
        // 标点走 OEM 物理键：GPUI 报的是字符，正/反两态都归到同一物理键
        "-" | "_" => 0xBD,
        "=" | "+" => 0xBB,
        "," | "<" => 0xBC,
        "." | ">" => 0xBE,
        "/" | "?" => 0xBF,
        ";" | ":" => 0xBA,
        "'" | "\"" => 0xDE,
        "[" | "{" => 0xDB,
        "]" | "}" => 0xDD,
        "\\" | "|" => 0xDC,
        "`" | "~" => 0xC0,
        "!" => 0x31,
        "@" => 0x32,
        "#" => 0x33,
        "$" => 0x34,
        "%" => 0x35,
        "^" => 0x36,
        "&" => 0x37,
        "*" => 0x38,
        "(" => 0x39,
        ")" => 0x30,
        _ => return None,
    })
}

/// GPUI 按键（键名 + 修饰位）→ accelerator。返回 None = 不满足条件
/// （缺 Ctrl/Cmd/Alt，或键名不支持）——设置页保持捕获态并提示。
pub(crate) fn accelerator_from_keystroke(
    key: &str,
    ctrl_or_cmd: bool,
    alt: bool,
    shift: bool,
) -> Option<String> {
    if !ctrl_or_cmd && !alt {
        return None;
    }
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    // 键名 → 写进 accelerator 的裸键段（字母大写、功能键大写，其余沿用键名）
    let bare = match key.to_lowercase().as_str() {
        c if c.chars().count() == 1 => c.to_uppercase(),
        "space" => "Space".to_string(),
        other => {
            let mut chars = other.chars();
            chars.next()?.to_uppercase().collect::<String>() + chars.as_str()
        }
    };
    // 认不出来的键名（vk_of 无解）当场拒掉，不留「保存了却注册不上」的坑
    vk_of(&bare)?;
    let mut parts: Vec<String> = Vec::new();
    if ctrl_or_cmd {
        parts.push("CommandOrControl".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    parts.push(bare);
    Some(parts.join("+"))
}

// —— macOS 注册参数（Carbon `RegisterEventHotKey`）——
//
// 只在 macOS 目标编译：Carbon 修饰位与 macOS 虚拟键码的唯一使用者是
// `platform/macos.rs`，Windows 上它们没有任何调用点（编译进来就是死代码）。

/// Carbon 修饰位（HIToolbox `Events.h` 的 cmdKey/shiftKey/optionKey）。
#[cfg(target_os = "macos")]
pub(crate) const CARBON_CMD: u32 = 0x0100;
#[cfg(target_os = "macos")]
pub(crate) const CARBON_SHIFT: u32 = 0x0200;
#[cfg(target_os = "macos")]
pub(crate) const CARBON_OPTION: u32 = 0x0800;

/// `MOD_*` 位 → Carbon 修饰位。
///
/// `CommandOrControl` 在 macOS 上落 **Command**（Tauri accelerator 同语义：
/// Windows 是 Ctrl、macOS 是 Cmd）；Alt 落 Option。`MOD_NOREPEAT` 等位忽略。
#[cfg(target_os = "macos")]
pub(crate) fn carbon_mods(mods: u32) -> u32 {
    let mut bits = 0;
    if mods & (MOD_CONTROL | MOD_WIN) != 0 {
        bits |= CARBON_CMD;
    }
    if mods & MOD_ALT != 0 {
        bits |= CARBON_OPTION;
    }
    if mods & MOD_SHIFT != 0 {
        bits |= CARBON_SHIFT;
    }
    bits
}

/// Win32 虚拟键码 → macOS 虚拟键码（ANSI 物理键位表）。
///
/// 只在 macOS 目标编译：macOS 侧注册前过一遍，认不出的键直接报注册失败，
/// 不留下「注册成功却从不触发」的假象。`Insert` 在 macOS 键盘上不存在、
/// F21–F24 超出 ANSI 表——返回 `None`。
#[cfg(target_os = "macos")]
pub(crate) fn mac_keycode(vk: u32) -> Option<u32> {
    let code = match vk {
        // 字母
        0x41 => 0x00, // A
        0x53 => 0x01, // S
        0x44 => 0x02, // D
        0x46 => 0x03, // F
        0x48 => 0x04, // H
        0x47 => 0x05, // G
        0x5A => 0x06, // Z
        0x58 => 0x07, // X
        0x43 => 0x08, // C
        0x56 => 0x09, // V
        0x42 => 0x0B, // B
        0x51 => 0x0C, // Q
        0x57 => 0x0D, // W
        0x45 => 0x0E, // E
        0x52 => 0x0F, // R
        0x59 => 0x10, // Y
        0x54 => 0x11, // T
        0x4F => 0x1F, // O
        0x55 => 0x20, // U
        0x49 => 0x22, // I
        0x50 => 0x23, // P
        0x4C => 0x25, // L
        0x4A => 0x26, // J
        0x4B => 0x28, // K
        0x4E => 0x2D, // N
        0x4D => 0x2E, // M
        // 数字
        0x31 => 0x12, // 1
        0x32 => 0x13, // 2
        0x33 => 0x14, // 3
        0x34 => 0x15, // 4
        0x36 => 0x16, // 6
        0x35 => 0x17, // 5
        0x39 => 0x19, // 9
        0x37 => 0x1A, // 7
        0x38 => 0x1C, // 8
        0x30 => 0x1D, // 0
        // 标点（物理键）
        0xBD => 0x1B, // -
        0xBB => 0x18, // =
        0xDB => 0x21, // [
        0xDD => 0x1E, // ]
        0xDC => 0x2A, // \
        0xBA => 0x29, // ;
        0xDE => 0x27, // '
        0xBC => 0x2B, // ,
        0xBE => 0x2F, // .
        0xBF => 0x2C, // /
        0xC0 => 0x32, // `
        // 导航
        0x20 => 0x31, // Space
        0x09 => 0x30, // Tab
        0x0D => 0x24, // Enter
        0x1B => 0x35, // Escape
        0x2E => 0x33, // Delete（macOS 上是退格键）
        0x24 => 0x73, // Home
        0x23 => 0x77, // End
        0x21 => 0x74, // PageUp
        0x22 => 0x79, // PageDown
        0x25 => 0x7B, // Left
        0x27 => 0x7C, // Right
        0x28 => 0x7D, // Down
        0x26 => 0x7E, // Up
        // 功能键
        0x70 => 0x7A, // F1
        0x71 => 0x78, // F2
        0x72 => 0x63, // F3
        0x73 => 0x76, // F4
        0x74 => 0x60, // F5
        0x75 => 0x61, // F6
        0x76 => 0x62, // F7
        0x77 => 0x64, // F8
        0x78 => 0x65, // F9
        0x79 => 0x6D, // F10
        0x7A => 0x67, // F11
        0x7B => 0x6F, // F12
        0x7C => 0x69, // F13
        0x7D => 0x6B, // F14
        0x7E => 0x71, // F15
        0x7F => 0x6A, // F16
        0x80 => 0x40, // F17
        0x81 => 0x4F, // F18
        0x82 => 0x50, // F19
        0x83 => 0x5A, // F20
        // 小键盘
        0x60 => 0x52, // Num0
        0x61 => 0x53, // Num1
        0x62 => 0x54, // Num2
        0x63 => 0x55, // Num3
        0x64 => 0x56, // Num4
        0x65 => 0x57, // Num5
        0x66 => 0x58, // Num6
        0x67 => 0x59, // Num7
        0x68 => 0x5B, // Num8
        0x69 => 0x5C, // Num9
        // Insert（0x2D）无对应物理键；F21–F24（0x84..0x87）超出 ANSI 表
        _ => return None,
    };
    Some(code)
}
