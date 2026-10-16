//! 老板键组合的纯函数（无副作用、无 crate 依赖——集成测试按源文件引入，
//! 见 `tests/boss_key.rs`）。
//!
//! 形态与 Tauri 完全一致：tauri accelerator 串（如 `"CommandOrControl+Alt+H"`，
//! 捕获自键盘物理键、末段是裸键名），空串 = 未启用。注册参数（修饰位 +
//! Win32 虚拟键码）也在这里算，`win32` 只负责把参数交给系统。

/// 修饰键位（Win32 `RegisterHotKey` 的 fsModifiers 值；与系统 ABI 对齐）。
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
        return Err("老板键为空".to_string());
    };
    let mut bits = 0u32;
    for m in mods {
        let m = m.to_lowercase();
        bits |= match m.as_str() {
            "commandorcontrol" | "ctrl" | "control" | "cmd" | "command" | "meta" => MOD_CONTROL,
            "super" | "win" => MOD_WIN,
            "alt" | "option" => MOD_ALT,
            "shift" => MOD_SHIFT,
            other => return Err(format!("不认识的修饰键「{other}」")),
        };
    }
    // 全局快捷键必须带 Ctrl/Cmd 或 Alt（Tauri 侧同口径：防劫持打字）
    if bits & (MOD_CONTROL | MOD_ALT) == 0 {
        return Err("全局快捷键需要 Ctrl/Cmd 或 Alt 修饰".to_string());
    }
    let vk = vk_of(core).ok_or_else(|| format!("暂不支持这个键「{core}」"))?;
    Ok(Hotkey {
        mods: bits | MOD_NOREPEAT,
        vk,
    })
}

/// `MOD_NOREPEAT`（0x4000）：按住不连发——老板键是切换语义，连发会把窗口
/// 藏了又显。
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
