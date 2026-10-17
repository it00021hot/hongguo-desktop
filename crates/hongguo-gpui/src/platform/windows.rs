//! Windows 平台实现：Win32 落窗/置顶/藏显（底座在 [`crate::win32`]）。
//!
//! 坐标空间是**物理像素、左上原点**；内容区与外框之间有四向边框偏移，
//! 落窗/快照都按外框做（gpui 的 `bounds()` 是内容区，直接当外框用会每圈
//! 把窗口缩一圈）。

use gpui_kit::Window;

use crate::win32::{
    HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, SW_MAXIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, border_offset, hwnd_of, set_window_pos, set_window_pos_raw, show_window,
    window_rect, work_area,
};

use super::{MINI_H, MINI_MARGIN, MINI_W, OuterRect, WindowSnapshot};

/// 壳级轮询是否可用（全局快捷键 + 第二次启动前置窗口）。
pub(crate) const SHELL_POLLER: bool = true;

/// 注册全局快捷键（[`crate::boss_key::accel`] 算好的修饰位 + 虚拟键码）。
pub(crate) fn register_hotkey(mods: u32, vk: u32) -> Result<(), String> {
    crate::win32::register_hotkey(mods, vk)
}

/// 注销全局快捷键。
pub(crate) fn unregister_hotkey() {
    crate::win32::unregister_hotkey();
}

/// 老板键是否被按下过（读后清）。
pub(crate) fn take_hotkey_fired() -> bool {
    crate::win32::take_hotkey_fired()
}

/// 第二次启动是否请求了前置窗口（读后复位）。
pub(crate) fn take_focus_request() -> bool {
    crate::single_instance::take_focus_request()
}

/// 最小化态还原（第二次启动前置时的「显示 + 还原」）。
pub(crate) fn restore_from_minimized(window: &Window) {
    if let Some(hwnd) = hwnd_of(window)
        && crate::win32::is_iconic(hwnd)
    {
        crate::win32::show_window(hwnd, crate::win32::SW_RESTORE);
    }
}

/// 快照窗口外框与最大化态。
pub(crate) fn snapshot(window: &Window) -> Option<WindowSnapshot> {
    let (x, y, w, h) = window_rect(hwnd_of(window)?)?;
    Some(WindowSnapshot {
        rect: OuterRect::new(x as f64, y as f64, w as f64, h as f64),
        maximized: window.is_maximized(),
    })
}

/// 当前外框（逻辑像素，左上原点）。
pub(crate) fn current_rect(window: &Window) -> Option<OuterRect> {
    snapshot(window).map(|s| s.rect)
}

/// 小屏：缩成 480×270 并落到工作区右下角。
pub(crate) fn place_mini(window: &Window, cx: &gpui_kit::App) -> Option<()> {
    let hwnd = hwnd_of(window)?;
    let scale = window.scale_factor() as f64;
    let (left, top, right, bottom) = work_area(hwnd, scale)?;
    // 小工作区（外接竖屏等）放不下就压到放得下为止；手动 min 而非 clamp：
    // 极小工作区下 min>max 时 clamp 会 panic（Tauri 同注释）
    let w = MINI_W.min(right - left - 2.0 * MINI_MARGIN);
    let h = MINI_H.min(bottom - top - 2.0 * MINI_MARGIN);
    let x = (right - w - MINI_MARGIN).max(left);
    let y = (bottom - h - MINI_MARGIN).max(top);
    // SetWindowPos 设的是外框：内容要 480×270（Tauri set_size 语义），
    // 外框 = 内容矩形按四向边偏移外扩（位置也要减去左/顶偏移，否则底边
    // 会越过工作区压到任务栏上）
    let (off_l, off_t, off_r, off_b) = border_offset(hwnd);
    let maximized = window.is_maximized();
    defer_window_op(window, cx, move || {
        // 最大化态直接 SetWindowPos 会与 maximize 摆放打架：先还原到窗口态
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
    Some(())
}

/// 还原到快照（先落位、后最大化）。
pub(crate) fn restore(window: &Window, cx: &gpui_kit::App, snap: WindowSnapshot) {
    let Some(hwnd) = hwnd_of(window) else {
        return;
    };
    let OuterRect { x, y, w, h } = snap.rect;
    defer_window_op(window, cx, move || {
        set_window_pos_raw(
            hwnd,
            HWND_TOP,
            x as i32,
            y as i32,
            w as i32,
            h as i32,
            SWP_NOACTIVATE,
        );
        if snap.maximized {
            show_window(hwnd, SW_MAXIMIZE);
        }
    });
}

/// 置顶（窗口层级）。
pub(crate) fn set_topmost(window: &Window, cx: &gpui_kit::App, on: bool) -> bool {
    let Some(hwnd) = hwnd_of(window) else {
        return false;
    };
    let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
    defer_window_op(window, cx, move || {
        set_window_pos(
            hwnd,
            after,
            0.0,
            0.0,
            0.0,
            0.0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    });
    true
}

/// 竖屏几何：当前内容矩形 + 工作区（逻辑像素）→ 目标外框（逻辑像素）。
///
/// `fit_rect` 吃 top-left 语义；本平台本来就是，直接换算内容区。
pub(crate) fn portrait_geometry(
    window: &Window,
    aspect: f64,
    full: bool,
) -> Option<(f64, f64, f64, f64)> {
    use crate::portrait::geom::{FitInput, fit_rect};

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
    Some(fit_rect(FitInput {
        aspect,
        full,
        cur,
        work: (left, top, right, bottom),
    }))
}

/// 落窗：SetWindowPos 设的是外框，内容尺寸要按四向边偏移外扩。
pub(crate) fn apply_frame(
    window: &Window,
    cx: &gpui_kit::App,
    (x, y, w, h): (f64, f64, f64, f64),
) {
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

/// 直接把窗口落到给定外框（竖屏铺满档退出还原用）。
pub(crate) fn restore_frame(window: &Window, cx: &gpui_kit::App, rect: OuterRect) {
    let Some(hwnd) = hwnd_of(window) else {
        return;
    };
    defer_window_op(window, cx, move || {
        set_window_pos_raw(
            hwnd,
            HWND_TOP,
            rect.x as i32,
            rect.y as i32,
            rect.w as i32,
            rect.h as i32,
            SWP_NOACTIVATE,
        );
    });
}

/// 窗口矩形（与 [`cursor_pos`] 同一坐标空间：物理像素、左上原点）。
pub(crate) fn window_rect(window: &Window) -> Option<(f64, f64, f64, f64)> {
    let (x, y, w, h) = crate::win32::window_rect(hwnd_of(window)?)?;
    Some((x as f64, y as f64, w as f64, h as f64))
}

/// 全局光标位置（物理像素、左上原点）。
pub(crate) fn cursor_pos(_window: &Window) -> Option<(f64, f64)> {
    crate::win32::cursor_pos().map(|(x, y)| (x as f64, y as f64))
}

pub(crate) fn is_visible(window: &Window) -> bool {
    hwnd_of(window).is_some_and(crate::win32::is_window_visible)
}

pub(crate) fn is_minimized(window: &Window) -> bool {
    hwnd_of(window).is_some_and(crate::win32::is_iconic)
}

pub(crate) fn hide(window: &Window) {
    if let Some(hwnd) = hwnd_of(window) {
        crate::win32::hide(hwnd);
    }
}

pub(crate) fn show_no_activate(window: &Window) {
    if let Some(hwnd) = hwnd_of(window) {
        crate::win32::show_no_activate(hwnd);
    }
}

pub(crate) fn show_foreground(window: &Window) {
    if let Some(hwnd) = hwnd_of(window) {
        crate::win32::show_foreground(hwnd);
    }
}

/// 当前窗口是否可见（老板键切换判定用；与 [`is_visible`] 同源）。
pub(crate) fn visibility(window: &Window) -> bool {
    is_visible(window)
}

/// 把窗口操作派发到执行器（主线程）再执行。
///
/// **不能在调用点同步做**：这些函数都从点击回调里进来，而点击回调正嵌在
/// 窗口过程的一次 `handle.update` 里；同步 `SetWindowPos` 会让 WM_SIZE
/// **同步重入**同一个 update——重入被 gpui 拒绝（`log_err` 吞掉），视口
/// 从此停在旧尺寸（实测舞台停在 1024×680、控制条画到 270 高的窗口外）。
/// gpui 自己的 `resize()` 也是把 SetWindowPos 派发到执行器绕开这一点。
fn defer_window_op(window: &Window, cx: &gpui_kit::App, op: impl FnOnce() + Send + 'static) {
    window.spawn(cx, async move |_| op()).detach();
}
