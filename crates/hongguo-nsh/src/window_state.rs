//! 跨启动的窗口几何记忆（对齐 Tauri 壳 `window_state.rs`）：三窗态各记
//! 各的槽位（常态 / 小屏 / 竖屏），移动/缩放防抖落盘 + 关窗即存，启动时
//! 恢复常态槽。小屏与竖屏的几何是该形态自己的记忆——切形态互不覆盖，
//! 下次进同一形态回上次的位置与大小。数据根与 Tauri 壳平级独立
//! （`%APPDATA%\hongguo-nsh\window-state.json`），档格式同 v2 三槽。
//!
//! 与 Tauri 版的两处差异：几何按**逻辑点**存（本壳全部窗口 API 都是点，
//! 没有 Tauri 的物理/逻辑双轨）；「最大化/全屏中不落盘」里全屏由
//! `platform_ext` 的全屏标记判断（nsh 没有最大化/最小化事件面）。

use serde::{Deserialize, Serialize};

use hongguo_store::paths;

/// 数据目录下的文件名（与 Tauri 壳同名同格式，互不共用数据根）。
const FILE_NAME: &str = "window-state.json";

/// 落盘地板：三形态合法最小值里 mini 的 360×180 最小，拿它当全局地板
/// （过渡态会读出塌缩尺寸，写进去会毒化该形态的记忆）。
const MIN_SAVE_W: f32 = 360.0;
const MIN_SAVE_H: f32 = 180.0;

/// 窗口几何（逻辑点；本壳的 SetWindowPos / setContentSize 都按点算）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct Geometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 窗态槽位路由（与 `platform_ext::WindowMode` 一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Normal,
    Mini,
    Portrait,
}

/// 档文件形态：空槽位不写键。
#[derive(Serialize, Deserialize, Default)]
struct Slots {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normal: Option<Geometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mini: Option<Geometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    portrait: Option<Geometry>,
}

fn path() -> std::path::PathBuf {
    paths::data_dir().join(FILE_NAME)
}

fn load_slots() -> Slots {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 把一份几何写进指定槽位（读整档 → 改一个槽 → 临时文件 + rename 原子
/// 替换；失败只记日志——几何记忆丢一次无伤大雅）。塌缩几何不进槽位。
pub(crate) fn store(mode: Mode, g: Geometry) {
    if g.width < MIN_SAVE_W || g.height < MIN_SAVE_H {
        log::info!(
            "[Window] 窗口几何塌缩（{:.0}×{:.0}），不落盘——等下一个稳定值",
            g.width,
            g.height
        );
        return;
    }
    let mut slots = load_slots();
    match mode {
        Mode::Normal => slots.normal = Some(g),
        Mode::Mini => slots.mini = Some(g),
        Mode::Portrait => slots.portrait = Some(g),
    }
    let Ok(text) = serde_json::to_string(&slots) else {
        return;
    };
    let tmp = path().with_extension("tmp");
    if std::fs::write(&tmp, text)
        .and_then(|_| std::fs::rename(&tmp, path()))
        .is_err()
    {
        log::warn!("[Window] 窗口几何写盘失败（下次启动回默认）");
    }
}

/// 读指定窗态的槽位；命中校验交给调用方（屏幕相交要在有窗口句柄的线程做）。
pub(crate) fn slot(mode: Mode) -> Option<Geometry> {
    let slots = load_slots();
    let g = match mode {
        Mode::Normal => slots.normal,
        Mode::Mini => slots.mini,
        Mode::Portrait => slots.portrait,
    }?;
    (g.width > 0.0 && g.height > 0.0).then_some(g)
}
