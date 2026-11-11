//! 跨启动的窗口几何记忆：移动/缩放防抖落盘 + 关闭即存，启动时恢复主窗几何到数据目录 `window-state.json`（GPUI 壳的 ui_prefs 同语义，双壳各自数据根互不共用）。
//!
//! 三种窗态各记各的槽位（常态 / 小屏 / 竖屏，切形态互不覆盖，下次进同一形态回上次位置）；最大化/全屏/最小化中的几何归系统管，不进任何槽位（Windows 最小化会发 Resized、inner_size 可能为 0×0，写进记忆等于清空）。

use serde::{Deserialize, Serialize};

use hongguo_store::paths;

/// 数据目录下的文件名。
const FILE_NAME: &str = "window-state.json";

/// 常态记忆的尺寸下限（window_cmd 的 MAIN_MIN 同款）：小于它视为坏档
/// 残留，不恢复也不覆盖。
const MIN_W: f64 = 480.0;
const MIN_H: f64 = 270.0;

/// 落盘地板：过渡态会读出塌缩尺寸（Windows 最小化/形态切换竞速可为 0×0），写进去会毒化该形态的记忆——之后每次进形态都被恢复判定拒掉。三形态合法最小值里 mini 的 360×180 最小，拿它当全局地板。
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
    if hongguo_common::offline_qa::qa_write(&tmp, text.as_bytes())
        .and_then(|_| hongguo_common::offline_qa::qa_rename(&tmp, &path()))
        .is_err()
    {
        log::warn!("[Window] 窗口几何写盘失败（下次启动回默认）");
    }
}

/// 显示器工作区 `(x, y, w, h)`，物理像素，与 `Geometry` 同空间。
type WorkArea = (i32, i32, i32, i32);

/// 当前几何按当前窗态路由落盘（saver 防抖与 CloseRequested 共用）：小屏中记小屏槽、竖屏中记竖屏槽。最大化/全屏/最小化中的几何不进任何槽位；塌缩尺寸过 MIN_SAVE 地板不写——宁可丢一拍，不毒化槽位。
///
/// 窗口尚未 `visible` 时不落盘：启动时 `restore` 的 `set_size`/`set_position` 在
/// macOS/Windows 上是异步入队的，saver 若在中间态取样会把「贴底窗被 resize
/// 锚点拽歪」的坐标写进档，下一轮又被校验丢掉（issue #15）。
pub fn save(win: &tauri::WebviewWindow) {
    if !win.is_visible().unwrap_or(false)
        || win.is_maximized().unwrap_or(false)
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

fn monitor_work_areas(win: &tauri::WebviewWindow) -> Vec<WorkArea> {
    win.available_monitors()
        .map(|monitors| {
            monitors
                .iter()
                .map(|m| {
                    let wa = m.work_area();
                    (
                        wa.position.x,
                        wa.position.y,
                        wa.size.width as i32,
                        wa.size.height as i32,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `g` 与工作区是否相交（GPUI `window_restore` 同口径；比「完整落在」宽松，
/// 影子边/异步恢复的轻微越界仍算有效记忆）。
fn intersects_work_area(g: Geometry, (wx, wy, ww, wh): WorkArea) -> bool {
    let (gx2, gy2) = (g.x + g.width as i32, g.y + g.height as i32);
    g.x < wx + ww && gx2 > wx && g.y < wy + wh && gy2 > wy
}

/// 把几何钳进最合适的工作区：优先包含窗口中心的屏，否则交面积最大的，
/// 再否则主屏（列表首项）。贴边越界只挪回可见，不丢记忆（issue #15）。
fn clamp_to_work_areas(g: Geometry, areas: &[WorkArea]) -> Geometry {
    let Some(wa) = pick_work_area(g, areas) else {
        return g;
    };
    let (wx, wy, ww, wh) = wa;
    let gw = g.width as i32;
    let gh = g.height as i32;
    let max_x = (wx + ww - gw).max(wx);
    let max_y = (wy + wh - gh).max(wy);
    Geometry {
        x: g.x.clamp(wx, max_x),
        y: g.y.clamp(wy, max_y),
        width: g.width,
        height: g.height,
    }
}

fn pick_work_area(g: Geometry, areas: &[WorkArea]) -> Option<WorkArea> {
    if areas.is_empty() {
        return None;
    }
    let cx = g.x.saturating_add_unsigned(g.width / 2);
    let cy = g.y.saturating_add_unsigned(g.height / 2);
    if let Some(&wa) = areas.iter().find(|&&(x, y, w, h)| {
        x <= cx && cx < x.saturating_add(w) && y <= cy && cy < y.saturating_add(h)
    }) {
        return Some(wa);
    }
    areas
        .iter()
        .copied()
        .filter(|&wa| intersects_work_area(g, wa))
        .max_by_key(|&(wx, wy, ww, wh)| {
            let x0 = g.x.max(wx);
            let y0 = g.y.max(wy);
            let x1 = (g.x + g.width as i32).min(wx + ww);
            let y1 = (g.y + g.height as i32).min(wy + wh);
            (x1 - x0).max(0) as i64 * (y1 - y0).max(0) as i64
        })
        .or_else(|| areas.first().copied())
}

/// 读指定窗态的记忆：尺寸过小丢弃；否则钳进当前工作区再恢复——越界只挪回
/// 可见，不再整份作废（「贴左下角重启下移出屏 → 再开被重置」的闭环）。
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
    let areas = monitor_work_areas(win);
    let clamped = clamp_to_work_areas(g, &areas);
    if clamped != g {
        log::info!(
            "[Window] {mode:?} 几何记忆 ({},{}) {}×{} 越出工作区，钳到 ({},{})",
            g.x,
            g.y,
            g.width,
            g.height,
            clamped.x,
            clamped.y
        );
    }
    Some(clamped)
}

/// 把几何落回窗口（物理坐标）。先 size 后 position：macOS `setContentSize`
/// 保 AppKit 左下原点，先改尺寸会暂时拽走 top-left，必须再 set_position 钉回。
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

    /// 1080p 去菜单栏后的工作区（物理像素）：y 从 30 起、高 1050。
    const WA: WorkArea = (0, 30, 1920, 1050);

    #[test]
    fn clamp_pulls_bottom_overflow_back_into_work_area() {
        // 贴底后下移出屏：底边 240+840=1080 > 工作区底 1080？工作区底=30+1050=1080，
        // 再下移 40 → y=280 时底=1120 越界。
        let g = Geometry {
            x: 0,
            y: 280,
            width: 1280,
            height: 840,
        };
        let got = clamp_to_work_areas(g, &[WA]);
        assert_eq!(got.y, 30 + 1050 - 840, "底边钳到工作区底");
        assert_eq!(got.x, 0);
        assert_eq!((got.width, got.height), (1280, 840));
    }

    #[test]
    fn clamp_keeps_flush_bottom_left_unchanged() {
        let g = Geometry {
            x: 0,
            y: 30 + 1050 - 840,
            width: 1280,
            height: 840,
        };
        assert_eq!(clamp_to_work_areas(g, &[WA]), g);
    }

    #[test]
    fn clamp_uses_primary_when_memory_is_on_unplugged_display() {
        let g = Geometry {
            x: 5000,
            y: 5000,
            width: 800,
            height: 600,
        };
        let got = clamp_to_work_areas(g, &[WA]);
        // 完全飞出时按轴钳到最近边：右下角 (1920-800, 30+1050-600)
        assert_eq!(got.x, 1920 - 800);
        assert_eq!(got.y, 30 + 1050 - 600);
    }

    #[test]
    fn intersects_accepts_partial_overlap() {
        let g = Geometry {
            x: -100,
            y: 100,
            width: 800,
            height: 600,
        };
        assert!(intersects_work_area(g, WA));
        assert!(!intersects_work_area(
            Geometry {
                x: -900,
                y: 100,
                width: 800,
                height: 600,
            },
            WA
        ));
    }
}
