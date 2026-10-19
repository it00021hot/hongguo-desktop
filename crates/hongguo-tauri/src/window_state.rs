//! 跨启动的窗口几何记忆（issue #11）：移动/缩放防抖落盘 + 关闭即存，
//! 启动时恢复主窗几何到数据目录的 `window-state.json`。GPUI 壳的
//! ui_prefs `win_*` 同语义（双壳各自的数据根，互不共用）。
//!
//! 两个刻意不记：小屏/竖屏这类会话瞬态的几何（[`in_transient_mode`]，
//! 退出瞬态自有快照恢复）；最大化/全屏的几何（覆盖上次的窗态记忆会让
//! 「下次开窗还是上次那个窗口」失真）。

use serde::{Deserialize, Serialize};

use hongguo_store::paths;

/// 数据目录下的文件名。
const FILE_NAME: &str = "window-state.json";

/// 记忆尺寸下限（window_cmd 的 MAIN_MIN 同款）：小于它视为瞬态残留，
/// 不恢复也不覆盖。
const MIN_W: f64 = 480.0;
const MIN_H: f64 = 270.0;

/// 窗口几何（物理像素——与 `outer_position`/`inner_size` 的读出语义
/// 一致，恢复用 `PhysicalPosition`/`PhysicalSize` 原样还原）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Geometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn path() -> std::path::PathBuf {
    paths::data_dir().join(FILE_NAME)
}

/// 读上次几何；文件缺失/坏档返回 None（启动回 conf 的中置默认）。
fn load() -> Option<Geometry> {
    let text = std::fs::read_to_string(path()).ok()?;
    match serde_json::from_str::<Geometry>(&text) {
        Ok(g) if g.width > 0 && g.height > 0 => Some(g),
        _ => None,
    }
}

/// 当前几何快照；瞬态/最大化/全屏/最小化中返回 None（不覆盖上次记忆）。
///
/// 最小化也要排除：Windows 会为最小化发 Resized，此时 `inner_size` 可能是
/// 0×0；写进记忆后下次启动只能回默认，等于「记住窗口」被一次最小化清空。
fn snapshot(win: &tauri::WebviewWindow) -> Option<Geometry> {
    if super::commands::window_cmd::in_transient_mode() {
        return None;
    }
    if win.is_maximized().unwrap_or(false)
        || win.is_fullscreen().unwrap_or(false)
        || win.is_minimized().unwrap_or(false)
    {
        return None;
    }
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
        return None;
    };
    Some(Geometry {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    })
}

/// 把当前几何写盘（临时文件 + rename 原子替换；失败只记日志——几何
/// 记忆丢一次无伤大雅，不该惊动用户）。
pub fn save(win: &tauri::WebviewWindow) {
    let Some(g) = snapshot(win) else {
        return;
    };
    let Ok(text) = serde_json::to_string(&g) else {
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

/// 启动恢复：记忆在、尺寸不小于下限、且与任一显示器的工作区相交才
/// 应用——拔掉显示器/改过布局的旧坐标会把窗口丢到屏外找不回。
pub fn restore(win: &tauri::WebviewWindow) {
    let Some(g) = load() else {
        return;
    };
    if (g.width as f64) < MIN_W || (g.height as f64) < MIN_H {
        return;
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
        log::info!("[Window] 窗口几何记忆不在当前屏幕内，回中置默认");
        return;
    }
    let _ = win.set_size(tauri::PhysicalSize::new(g.width, g.height));
    let _ = win.set_position(tauri::PhysicalPosition::new(g.x, g.y));
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
