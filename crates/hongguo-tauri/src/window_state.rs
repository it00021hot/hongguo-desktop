//! 跨启动的窗口几何记忆（issue #11）：移动/缩放防抖落盘 + 关闭即存，
//! 启动时恢复主窗几何到数据目录的 `window-state.json`。GPUI 壳的
//! ui_prefs `win_*` 同语义（双壳各自的数据根，互不共用）。
//!
//! 三种窗态各记各的槽位（常态 / 小屏 / 竖屏，[`Mode`]）：小屏与竖屏的
//! 几何是该形态自己的记忆而不是瞬态——切形态互不覆盖，下次进同一形态
//! 回上次的位置与大小。最大化/全屏/最小化中的几何归系统管，不进任何
//! 槽位（覆盖记忆会让「下次开窗还是上次那个窗口」失真；Windows 最小化
//! 会发 Resized、inner_size 可能为 0×0，写进记忆等于清空）。

use serde::{Deserialize, Serialize};

use hongguo_store::paths;

/// 数据目录下的文件名。
const FILE_NAME: &str = "window-state.json";

/// 常态记忆的尺寸下限（window_cmd 的 MAIN_MIN 同款）：小于它视为坏档
/// 残留，不恢复也不覆盖。
const MIN_W: f64 = 480.0;
const MIN_H: f64 = 270.0;

/// 落盘地板：比任何合法窗口都小的几何不进槽位。过渡态会读出塌缩尺寸
/// （Windows 最小化/形态切换竞速，cc19e60 在单槽时代就记过 inner_size
/// 可能为 0×0），写进去会毒化该形态的记忆——之后每次进形态都被恢复
/// 判定拒掉回默认。三形态的合法最小值里 mini 的 360×180 最小（window_cmd
/// 的 MINI_MIN 同参），拿它当全局地板。
const MIN_SAVE_W: f64 = 360.0;
const MIN_SAVE_H: f64 = 180.0;

/// 窗口几何（物理像素——与 `outer_position`/`inner_size` 的读出语义
/// 一致，恢复用 `PhysicalPosition`/`PhysicalSize` 原样还原）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Geometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// 窗态：与 window_cmd 的形态快照静态一一对应，[`save`] 按它路由落盘
/// （判别见 [`super::commands::window_cmd::current_mode`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Normal,
    Mini,
    Portrait,
}

/// 全部槽位（内存态）。
#[derive(Debug, Default, PartialEq)]
struct Slots {
    normal: Option<Geometry>,
    mini: Option<Geometry>,
    portrait: Option<Geometry>,
}

/// 档文件的反序列化形态：v2 三槽；v1（issue #11 首版）只有常态一份平铺
/// 几何，读进 normal——released 版本已经写出过 v1 档。
#[derive(Debug, PartialEq, Deserialize)]
struct RawFile {
    #[serde(default)]
    normal: Option<Geometry>,
    #[serde(default)]
    mini: Option<Geometry>,
    #[serde(default)]
    portrait: Option<Geometry>,
    #[serde(default)]
    x: Option<i32>,
    #[serde(default)]
    y: Option<i32>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

/// v2 序列化形态：空槽位不写键，档面保持紧凑。
#[derive(Serialize)]
struct OutFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    normal: Option<Geometry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mini: Option<Geometry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    portrait: Option<Geometry>,
}

/// 尺寸非 0 才是可用记忆（0×0 是坏档残留）。
fn sane(g: &Geometry) -> bool {
    g.width > 0 && g.height > 0
}

/// 档文本 → 槽位。v2 以 `normal` 键判别；没有它但四个平铺键齐全则按 v1
/// 读进 normal；两者都不是（半写/手改坏档）回全空。
fn parse(text: &str) -> Slots {
    let Ok(raw) = serde_json::from_str::<RawFile>(text) else {
        return Slots::default();
    };
    let (normal, mini, portrait) = if raw.normal.is_some() {
        (raw.normal, raw.mini, raw.portrait)
    } else if let (Some(x), Some(y), Some(width), Some(height)) =
        (raw.x, raw.y, raw.width, raw.height)
    {
        (
            Some(Geometry {
                x,
                y,
                width,
                height,
            }),
            None,
            None,
        )
    } else {
        (None, None, None)
    };
    Slots {
        normal: normal.filter(sane),
        mini: mini.filter(sane),
        portrait: portrait.filter(sane),
    }
}

fn path() -> std::path::PathBuf {
    paths::data_dir().join(FILE_NAME)
}

fn load_slots() -> Slots {
    std::fs::read_to_string(path())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// 把一份几何写进指定槽位（读整档 → 改一个槽 → 临时文件 + rename 原子
/// 替换；失败只记日志——几何记忆丢一次无伤大雅，不该惊动用户）。
fn store(mode: Mode, g: Geometry) {
    let mut slots = load_slots();
    match mode {
        Mode::Normal => slots.normal = Some(g),
        Mode::Mini => slots.mini = Some(g),
        Mode::Portrait => slots.portrait = Some(g),
    }
    let Ok(text) = serde_json::to_string(&OutFile {
        normal: slots.normal,
        mini: slots.mini,
        portrait: slots.portrait,
    }) else {
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

/// 当前几何按当前窗态路由落盘（saver 防抖与 CloseRequested 共用）：小屏
/// 中记小屏槽、竖屏中记竖屏槽、常态记常态槽。最大化/全屏/最小化中的几何
/// 归系统管，不进任何槽位（见模块注释）；塌缩尺寸过 MIN_SAVE 地板不写
/// ——宁可丢一拍，不毒化槽位。
pub fn save(win: &tauri::WebviewWindow) {
    if win.is_maximized().unwrap_or(false)
        || win.is_fullscreen().unwrap_or(false)
        || win.is_minimized().unwrap_or(false)
    {
        return;
    }
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
        return;
    };
    if (size.width as f64) < MIN_SAVE_W || (size.height as f64) < MIN_SAVE_H {
        log::info!(
            "[Window] 窗口几何塌缩（{}×{}），不落盘——等下一个稳定值",
            size.width,
            size.height
        );
        return;
    }
    let mode = super::commands::window_cmd::current_mode();
    store(
        mode,
        Geometry {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        },
    );
}

/// 读指定窗态的记忆并校验：尺寸不小于 min（各形态 MIN 同款的「物理几何
/// 对逻辑下限」朴素比较，v1 起的既定口径），且与任一显示器的工作区相交
/// ——拔掉显示器/改过布局的旧坐标会把窗口丢到屏外找不回。命中不了记
/// 日志：「记忆为什么没生效」是这功能最高频的排查诉求。
fn load_valid(win: &tauri::WebviewWindow, mode: Mode, min: (f64, f64)) -> Option<Geometry> {
    let slots = load_slots();
    let g = match mode {
        Mode::Normal => slots.normal,
        Mode::Mini => slots.mini,
        Mode::Portrait => slots.portrait,
    }?;
    if (g.width as f64) < min.0 || (g.height as f64) < min.1 {
        log::info!(
            "[Window] {mode:?} 几何记忆 {}×{} 小于下限 {min:?}，回默认",
            g.width,
            g.height
        );
        return None;
    }
    let (left, top, right, bottom) = (g.x, g.y, g.x + g.width as i32, g.y + g.height as i32);
    let on_screen = win
        .available_monitors()
        .map(|monitors| {
            monitors.iter().any(|m| {
                let wa = m.work_area();
                let (mx, my) = (wa.position.x, wa.position.y);
                let (mw, mh) = (wa.size.width as i32, wa.size.height as i32);
                mx < right && left < mx + mw && my < bottom && top < my + mh
            })
        })
        .unwrap_or(false);
    if !on_screen {
        log::info!("[Window] {mode:?} 几何记忆不在当前屏幕内，回默认");
        return None;
    }
    Some(g)
}

/// 把几何原样落回窗口（物理坐标）。
fn apply(win: &tauri::WebviewWindow, g: Geometry) {
    let _ = win.set_size(tauri::PhysicalSize::new(g.width, g.height));
    let _ = win.set_position(tauri::PhysicalPosition::new(g.x, g.y));
}

/// 读指定窗态的记忆，校验通过就落回窗口；返回是否命中（未命中时调用方
/// 回该形态的默认落位）。几何类型不出本模块。
pub(crate) fn apply_saved(win: &tauri::WebviewWindow, mode: Mode, min: (f64, f64)) -> bool {
    match load_valid(win, mode, min) {
        Some(g) => {
            apply(win, g);
            true
        }
        None => false,
    }
}

/// 启动恢复常态几何：窗口永远以常态开（形态是会话态），小屏/竖屏槽位
/// 在各自进形态时取用（window_cmd 的 enter_*）。
pub fn restore(win: &tauri::WebviewWindow) {
    apply_saved(win, Mode::Normal, (MIN_W, MIN_H));
}

/// 移动/缩放的防抖落盘：事件闭包只递增 epoch，saver 线程轮询到变化才
/// 快照写盘（拖动时每帧一个事件，逐事件写盘是浪费）。随进程退出终止。
pub fn spawn_saver(win: tauri::WebviewWindow, epoch: std::sync::Arc<std::sync::atomic::AtomicU64>) {
    use std::sync::atomic::Ordering;
    std::thread::spawn(move || {
        let mut seen = 0u64;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let now = epoch.load(Ordering::Acquire);
            if now == seen {
                continue;
            }
            seen = now;
            save(&win);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_three_v2_slots() {
        let slots = parse(
            r#"{"normal":{"x":1,"y":2,"width":800,"height":600},
                "mini":{"x":3,"y":4,"width":480,"height":270},
                "portrait":{"x":5,"y":6,"width":405,"height":840}}"#,
        );
        assert_eq!(
            slots.normal,
            Some(Geometry {
                x: 1,
                y: 2,
                width: 800,
                height: 600
            })
        );
        assert_eq!(
            slots.mini,
            Some(Geometry {
                x: 3,
                y: 4,
                width: 480,
                height: 270
            })
        );
        assert_eq!(
            slots.portrait,
            Some(Geometry {
                x: 5,
                y: 6,
                width: 405,
                height: 840
            })
        );
    }

    #[test]
    fn missing_v2_slots_read_as_none() {
        let slots = parse(r#"{"normal":{"x":1,"y":2,"width":800,"height":600}}"#);
        assert!(slots.normal.is_some());
        assert_eq!(slots.mini, None);
        assert_eq!(slots.portrait, None);
    }

    #[test]
    fn legacy_flat_file_reads_into_the_normal_slot() {
        let slots = parse(r#"{"x":10,"y":20,"width":900,"height":600}"#);
        assert_eq!(
            slots.normal,
            Some(Geometry {
                x: 10,
                y: 20,
                width: 900,
                height: 600
            })
        );
        assert_eq!(slots.mini, None);
        assert_eq!(slots.portrait, None);
    }

    #[test]
    fn incomplete_legacy_keys_discard_the_file() {
        assert_eq!(parse(r#"{"x":10,"width":900}"#), Slots::default());
    }

    #[test]
    fn corrupt_file_falls_back_to_empty() {
        assert_eq!(parse("not json"), Slots::default());
        assert_eq!(parse(r#"{"normal":{"x":1}}"#), Slots::default());
        assert_eq!(
            parse(r#"{"normal":{"x":1,"y":2,"width":0,"height":600}}"#),
            Slots::default()
        );
    }

    #[test]
    fn empty_slots_are_omitted_and_round_trip() {
        let text = serde_json::to_string(&OutFile {
            normal: Some(Geometry {
                x: 1,
                y: 2,
                width: 800,
                height: 600,
            }),
            mini: None,
            portrait: None,
        })
        .unwrap();
        assert_eq!(text, r#"{"normal":{"x":1,"y":2,"width":800,"height":600}}"#);
        let slots = parse(&text);
        assert!(slots.normal.is_some());
        assert_eq!(slots.mini, None);
    }
}
