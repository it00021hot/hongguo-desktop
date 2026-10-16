//! 竖屏播放的窗口几何（Tauri `window_cmd` 的 portrait 段对齐）：同一个窗口
//! 收窄成竖条、退出按快照还原——与小屏同一套机制（[`crate::mini`] 的快照
//! 类型与 Win32 原语直接复用）。
//!
//! 与 Tauri 的逐项对应：进入先退全屏（全屏窗口尺寸归系统管，收窄会被吞）、
//! 最小宽 360、普通档高度最多吃工作区高的 70%、最大化进竖屏直接进铺满档、
//! 铺满档退出还原普通档几何快照、切集/换清晰度导致比例变化 >1% 时重适配。
//! macOS 侧与小屏同缺口：只缩不挪（Apple FFI 后补）。

pub(crate) mod geom;

use gpui_kit::Window;

use crate::mini::SavedGeometry;
use geom::FitInput;

#[cfg(target_os = "windows")]
use crate::win32::{
    HWND_TOP, SW_RESTORE, SWP_NOACTIVATE, border_offset, hwnd_of, set_window_pos_raw, show_window,
    window_rect, work_area,
};

/// 铺满档退出时要还原的普通档几何（x, y, w, h，外框物理像素；Tauri
/// `PORTRAIT_FULL_RESTORE` 同语义）。
static FULL_RESTORE: std::sync::Mutex<Option<(i32, i32, i32, i32)>> = std::sync::Mutex::new(None);

/// 竖屏开着：HomeView 写，Shell 读（侧栏在竖屏里藏掉、顶栏保留——Tauri
/// app-shell 同款：`!miniScreen && !playerFullscreen && !portrait` 才渲染侧栏）。
static PORTRAIT_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_active(on: bool) {
    PORTRAIT_ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn is_active() -> bool {
    PORTRAIT_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 进入竖屏：快照 → 按比例收窄落位。几何不可得返回 `None`（调用方保持
/// 原状）。调用方负责幂等与「先退出其它紧凑形态」。
pub(crate) fn enter(window: &mut Window, aspect: f64, cx: &gpui_kit::App) -> Option<SavedGeometry> {
    let (saved, maximized) = crate::mini::snapshot(window)?;
    // 最大化进竖屏：退最大化直接进铺满档（Tauri enter 的 Ev + fit(full)）
    let full = maximized;
    *FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    fit(window, aspect, full, cx);
    Some(saved)
}

/// 按比例重算并落窗（换集/换清晰度的重适配与首次进入共用）。
pub(crate) fn fit(window: &Window, aspect: f64, full: bool, cx: &gpui_kit::App) {
    let Some((x, y, w, h)) = compute(window, aspect, full) else {
        return;
    };
    apply(window, cx, x, y, w, h);
}

/// 铺满档切换（Tauri `set_portrait_full` 同款）：普通→铺满先快照当前几何；
/// 铺满→普通还原快照（没有快照——最大化直接进来的——按普通档重算）。
pub(crate) fn set_full(window: &mut Window, aspect: f64, full: bool, cx: &gpui_kit::App) {
    if full {
        if let Some(rect) = outer_rect(window) {
            *FULL_RESTORE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(rect);
        }
        fit(window, aspect, true, cx);
        return;
    }
    let saved = FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    match saved {
        Some((x, y, w, h)) => {
            let Some(hwnd) = hwnd_or_none(window) else {
                return;
            };
            defer_window_op(window, cx, move || {
                set_window_pos_raw(hwnd, HWND_TOP, x, y, w, h, SWP_NOACTIVATE);
            });
        }
        None => fit(window, aspect, false, cx),
    }
}

/// 退出竖屏：还原进入前的几何与最大化态（丢掉的铺满快照不再需要）。
pub(crate) fn exit(window: &mut Window, saved: SavedGeometry, cx: &gpui_kit::App) {
    *FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    crate::mini::exit(window, cx, saved);
}

/// 当前窗口矩形（外框，物理像素）+ 工作区 → 竖屏矩形（逻辑像素）。
#[cfg(target_os = "windows")]
fn compute(window: &Window, aspect: f64, full: bool) -> Option<(f64, f64, f64, f64)> {
    let hwnd = hwnd_of(window)?;
    let scale = window.scale_factor() as f64;
    let (left, top, right, bottom) = work_area(hwnd, scale)?;
    let (rx, ry, rw, rh) = window_rect(hwnd)?;
    let (off_l, off_t, off_r, off_b) = border_offset(hwnd);
    // 外框 → 内容（Tauri 的 outer_position/inner_size 同量）
    let cur = (
        rx as f64 / scale + off_l as f64 / scale,
        ry as f64 / scale + off_t as f64 / scale,
        (rw - off_l - off_r) as f64 / scale,
        (rh - off_t - off_b) as f64 / scale,
    );
    Some(geom::fit_rect(FitInput {
        aspect,
        full,
        cur,
        work: (left, top, right, bottom),
    }))
}

#[cfg(not(target_os = "windows"))]
fn compute(window: &Window, aspect: f64, _full: bool) -> Option<(f64, f64, f64, f64)> {
    // macOS：gpui 无位置/工作区 API（Apple FFI 后补）——保持当前高、按比例
    // 收窄，位置交给系统（与小屏「只缩不挪」同口径）
    let b = window.bounds();
    let h = b.size.height.to_f64();
    Some((
        b.origin.x.to_f64(),
        b.origin.y.to_f64(),
        (h * aspect).round().max(geom::PORTRAIT_MIN_W),
        h,
    ))
}

/// 落窗：SetWindowPos 设的是外框，内容尺寸要按四向边偏移外扩（小屏同款）。
#[cfg(target_os = "windows")]
fn apply(window: &Window, cx: &gpui_kit::App, x: f64, y: f64, w: f64, h: f64) {
    let Some(hwnd) = hwnd_of(window) else {
        return;
    };
    let scale = window.scale_factor() as f64;
    let (off_l, off_t, off_r, off_b) = border_offset(hwnd);
    // 最大化态直接 SetWindowPos 会与 maximize 摆放打架：先还原到窗口态
    //（小屏 place_mini 同款守卫；最大化进竖屏在快照时已记成铺满档）
    let maximized = window.is_maximized();
    defer_window_op(window, cx, move || {
        if maximized {
            show_window(hwnd, SW_RESTORE);
        }
        set_window_pos_raw(
            hwnd,
            HWND_TOP,
            (x * scale) as i32 - off_l,
            (y * scale) as i32 - off_t,
            (w * scale) as i32 + off_l + off_r,
            (h * scale) as i32 + off_t + off_b,
            SWP_NOACTIVATE,
        );
    });
}

#[cfg(not(target_os = "windows"))]
fn apply(window: &Window, _cx: &gpui_kit::App, _x: f64, _y: f64, w: f64, h: f64) {
    window.resize(gpui_kit::Size {
        width: gpui_kit::Pixels::from(w as f32),
        height: gpui_kit::Pixels::from(h as f32),
    });
}

#[cfg(target_os = "windows")]
fn outer_rect(window: &Window) -> Option<(i32, i32, i32, i32)> {
    window_rect(hwnd_of(window)?)
}

#[cfg(not(target_os = "windows"))]
fn outer_rect(window: &Window) -> Option<(i32, i32, i32, i32)> {
    let b = window.bounds();
    Some((
        b.origin.x.to_f64() as i32,
        b.origin.y.to_f64() as i32,
        b.size.width.to_f64() as i32,
        b.size.height.to_f64() as i32,
    ))
}

#[cfg(target_os = "windows")]
fn hwnd_or_none(window: &Window) -> Option<isize> {
    hwnd_of(window)
}

#[cfg(not(target_os = "windows"))]
fn hwnd_or_none(_window: &Window) -> Option<isize> {
    None
}

/// 窗口操作派发到执行器再执行（理由同 [`crate::mini`]：点击回调里同步
/// `SetWindowPos` 会让 WM_SIZE 同步重入 gpui 的 update，被拒后视口停在旧尺寸）。
#[cfg(target_os = "windows")]
fn defer_window_op(window: &Window, cx: &gpui_kit::App, op: impl FnOnce() + Send + 'static) {
    window.spawn(cx, async move |_| op()).detach();
}
