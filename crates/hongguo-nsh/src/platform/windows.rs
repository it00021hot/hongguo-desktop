//! Windows 平台实现：单实例门闩、窗口枚举、几何、全屏、透明度、光标。
//!
//! 为了不自己养一套 Win32 声明，**直接用 nsh 转出的 `windows` crate**
//! （`nsh_platform_windows::windows`，0.62，feature 由 nsh 自己开）：类型、
//! 常量、函数签名都跟着上游走，nsh 升级换版本时这里不改。

use super::Geometry;
use std::cell::Cell;

use nsh_platform_windows::windows::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, POINT, RECT, WAIT_OBJECT_0},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
    System::Threading::{
        CreateEventW, CreateMutexW, GetCurrentProcessId, ResetEvent, SetEvent, WaitForSingleObject,
    },
    UI::{
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        WindowsAndMessaging::{
            EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetCursorPos, GetWindowLongW,
            GetWindowRect, GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsWindowVisible,
            LWA_ALPHA, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongW, SetWindowPos,
            ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WS_EX_LAYERED, WS_OVERLAPPEDWINDOW,
        },
    },
};
use nsh_platform_windows::windows::core::{BOOL, HSTRING};

/// nsh 注册的顶层窗口类名（`nsh-platform-windows/src/window.rs` 的 `CLASS`）。
const NSH_CLASS: &str = "NshWindow";

/// 全屏标记（Windows 实现自己的开关状态；macOS 以 styleMask 为准）。
static FULLSCREEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 命名内核对象（`HANDLE` 带裸指针，不是 `Send`；门闩要活到进程退出，
/// 所以按 `isize` 存进静态量，用时再转回句柄）。
static SINGLETON: std::sync::Mutex<Option<(isize, isize)>> = std::sync::Mutex::new(None);

fn handle(raw: isize) -> nsh_platform_windows::windows::Win32::Foundation::HANDLE {
    nsh_platform_windows::windows::Win32::Foundation::HANDLE(raw as *mut _)
}

pub fn claim_single_instance() -> bool {
    let mutex_name = HSTRING::from("HongguoNsh.SingleInstance");
    let event_name = HSTRING::from("HongguoNsh.SingleInstance.Focus");
    // SAFETY：两个句柄进程生命周期内保留（单实例门闩必须活到进程退出，不能关）。
    unsafe {
        let Ok(mutex) = CreateMutexW(None, true, &mutex_name) else {
            log::warn!("[Shell] 单实例互斥体创建失败，按独占启动继续");
            return true;
        };
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        let Ok(event) = CreateEventW(None, true, false, &event_name) else {
            log::warn!("[Shell] 前置事件创建失败，按独占启动继续");
            return true;
        };
        if already {
            let _ = SetEvent(event);
            log::info!("[Shell] 单实例：已有实例在跑，已请它前置窗口");
            return false;
        }
        *SINGLETON
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some((mutex.0 as isize, event.0 as isize));
        log::info!("[Shell] 单实例：已取得门闩");
        true
    }
}

/// 第二个实例是否请求过前置（读后复位）。
pub fn take_focus_request() -> bool {
    let raw = SINGLETON
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .map(|(_, event)| event)
        .unwrap_or(0);
    if raw == 0 {
        return false;
    }
    // SAFETY：句柄是本进程 claim 出来的事件。
    unsafe {
        if WaitForSingleObject(handle(raw), 0) != WAIT_OBJECT_0 {
            return false;
        }
        let _ = ResetEvent(handle(raw));
    }
    true
}

/// 已有实例收到前置请求后：显示、还原、聚焦。
pub fn bring_to_front() {
    let Some(hwnd) = find_window() else {
        log::warn!("[Shell] 前置窗口：没找到 NshWindow");
        return;
    };
    // SAFETY：句柄来自本进程窗口枚举。
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    log::info!("[Shell] 已前置窗口");
}

/// 当前窗口外框几何（逻辑点）；窗口不存在（关窗退出中）时 `None`。
pub fn window_rect() -> Option<Geometry> {
    let hwnd = find_window()?;
    let mut rect = RECT::default();
    // SAFETY：句柄来自本进程窗口枚举。
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    let dpi = dpi_of(hwnd) as f32 / 96.0;
    Some(Geometry {
        x: rect.left as f32 / dpi,
        y: rect.top as f32 / dpi,
        width: (rect.right - rect.left) as f32 / dpi,
        height: (rect.bottom - rect.top) as f32 / dpi,
    })
}

/// 显示器工作区（逻辑点，左/上/右/下）。
pub fn work_area() -> (f32, f32, f32, f32) {
    let Some(hwnd) = find_window() else {
        return (0.0, 0.0, 1280.0, 720.0);
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY：句柄来自本进程窗口枚举；结构体是本次调用自造的合法缓冲。
    let ok = unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        !monitor.is_invalid() && GetMonitorInfoW(monitor, &mut info).as_bool()
    };
    if !ok {
        return (0.0, 0.0, 1280.0, 720.0);
    }
    let dpi = dpi_of(hwnd) as f32 / 96.0;
    (
        info.rcWork.left as f32 / dpi,
        info.rcWork.top as f32 / dpi,
        info.rcWork.right as f32 / dpi,
        info.rcWork.bottom as f32 / dpi,
    )
}

/// 几何是否与任一显示器工作区相交（恢复校验：拔掉显示器的旧坐标会丢窗）。
pub fn on_any_monitor(g: &Geometry) -> bool {
    let Some(hwnd) = find_window() else {
        return false;
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY：句柄与结构体都是本次调用自造的合法值。
    let ok = unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        !monitor.is_invalid() && GetMonitorInfoW(monitor, &mut info).as_bool()
    };
    if !ok {
        return false;
    }
    let dpi = dpi_of(hwnd) as f32 / 96.0;
    let (left, top) = (g.x * dpi, g.y * dpi);
    let (right, bottom) = (left + g.width * dpi, top + g.height * dpi);
    let wa = info.rcWork;
    (wa.left as f32) < right
        && left < wa.right as f32
        && (wa.top as f32) < bottom
        && top < wa.bottom as f32
}

/// 应用内容尺寸 + 外框原点（点；内部按 dpi 换算成外框再落窗）。
pub fn apply_content_and_origin(w: f32, h: f32, x: f32, y: f32) {
    let Some(hwnd) = find_window() else {
        return;
    };
    let dpi = dpi_of(hwnd);
    let scale = dpi as f32 / 96.0;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: (w * scale).round() as i32,
        bottom: (h * scale).round() as i32,
    };
    // SAFETY：句柄来自本进程窗口枚举；结构体是本次调用自造的合法缓冲。
    unsafe {
        let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
        let ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
        let _ = AdjustWindowRectExForDpi(&mut rect, style, false, ex, dpi);
        let (outer_w, outer_h) = (rect.right - rect.left, rect.bottom - rect.top);
        let _ = SetWindowPos(
            hwnd,
            None,
            x.mul_add(scale, 0.0) as i32,
            y.mul_add(scale, 0.0) as i32,
            outer_w,
            outer_h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// 置顶开关（小窗控制条与播放页共用，Tauri `set_always_on_top` 同语义）。
pub fn set_always_on_top(top: bool) {
    let Some(hwnd) = find_window() else {
        return;
    };
    // SAFETY：句柄来自本进程窗口枚举；只改 Z 序，不动位置尺寸。
    unsafe {
        let after = if top { HWND_TOPMOST } else { HWND_NOTOPMOST };
        let _ = SetWindowPos(
            hwnd,
            Some(after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    log::info!("[Shell] 置顶 → {top}");
}

/// 是否处于全屏（本壳自己的标记，见 [`super::toggle_fullscreen`]）。
pub fn is_fullscreen() -> bool {
    FULLSCREEN.load(std::sync::atomic::Ordering::Acquire)
}

/// 全屏开关：清掉重叠窗口样式、铺满所在显示器；退出还原样式与原位。
pub fn toggle_fullscreen() {
    let Some(hwnd) = find_window() else {
        return;
    };
    static PLACEMENT: std::sync::Mutex<Option<(WINDOW_STYLE, RECT)>> = std::sync::Mutex::new(None);
    // SAFETY：句柄来自本进程窗口枚举；样式/位置读写与结构体都是本进程窗口。
    unsafe {
        let mut placement = PLACEMENT.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((style, rect)) = placement.take() {
            SetWindowLongW(hwnd, GWL_STYLE, style.0 as i32);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            FULLSCREEN.store(false, std::sync::atomic::Ordering::Release);
            log::info!("[Shell] 退出全屏");
            return;
        }
        let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let mon = info.rcMonitor;
        *placement = Some((style, rect));
        SetWindowLongW(hwnd, GWL_STYLE, (style.0 & !WS_OVERLAPPEDWINDOW.0) as i32);
        let _ = SetWindowPos(
            hwnd,
            None,
            mon.left,
            mon.top,
            mon.right - mon.left,
            mon.bottom - mon.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        FULLSCREEN.store(true, std::sync::atomic::Ordering::Release);
        log::info!("[Shell] 进入全屏");
    }
}

/// 无边框播放的窗口侧（Windows 本就无边框，唯一要做的是记住标记供
/// 其他平台分支参考；App chrome 的藏显由 `app.rs` 按信号裁决）。
pub fn set_frameless(_on: bool) {}

/// 窗口透明度（0..1，隐身模式用）：挂 `WS_EX_LAYERED` 后设 alpha；
/// 恢复时 alpha 回 1（layered 位留着不摘，避免闪烁）。
pub fn set_window_alpha(alpha: f32) {
    let Some(hwnd) = find_window() else {
        return;
    };
    // SAFETY：句柄来自本进程窗口枚举；alpha 钳在合法区间。
    unsafe {
        let ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
        if (ex & WS_EX_LAYERED).0 == 0 {
            SetWindowLongW(hwnd, GWL_EXSTYLE, (ex | WS_EX_LAYERED).0 as i32);
        }
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        let _ = SetLayeredWindowAttributes(
            hwnd,
            nsh_platform_windows::windows::Win32::Foundation::COLORREF(0),
            a,
            LWA_ALPHA,
        );
    }
}

/// 光标当前位置（窗口内容坐标，点）；不在本进程窗口内时返回 `None`。
///
/// nsh 0.1.1 没有指针移动事件，播放页的「动鼠标唤醒悬浮层」只能靠
/// 播放域低频轮询这个口子实现（见 `player::mod` 的 150ms 任务）。
/// `EnumWindows`/`GetCursorPos` 都不限线程，引擎线程轮询安全。
pub fn cursor_in_window() -> Option<(f32, f32)> {
    let hwnd = find_window()?;
    let mut pos = POINT::default();
    // SAFETY：两个结构体都是本次调用自造的合法缓冲，查询只读。
    unsafe {
        GetCursorPos(&mut pos).ok()?;
    }
    let mut rect = RECT::default();
    // SAFETY：句柄来自本进程窗口枚举。
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    if pos.x < rect.left || pos.x >= rect.right || pos.y < rect.top || pos.y >= rect.bottom {
        return None;
    }
    let dpi = dpi_of(hwnd) as f32 / 96.0;
    Some((
        (pos.x - rect.left) as f32 / dpi,
        (pos.y - rect.top) as f32 / dpi,
    ))
}

/// 本进程按类名找到的 nsh 顶层窗口。
pub fn find_window() -> Option<HWND> {
    thread_local! {
        /// `EnumWindows` 回调的收集槽（回调在本线程同步执行）。
        static FOUND: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    }

    unsafe extern "system" fn collect(hwnd: HWND, _param: LPARAM) -> BOOL {
        // SAFETY：句柄由系统传入，四个查询都是只读。
        unsafe {
            let mut pid = 0_u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != GetCurrentProcessId() || !IsWindowVisible(hwnd).as_bool() {
                return BOOL::from(true);
            }
            let mut buf = [0_u16; 64];
            let len = GetClassNameW(hwnd, &mut buf);
            if len <= 0 {
                return BOOL::from(true);
            }
            if String::from_utf16_lossy(&buf[..len as usize]) == NSH_CLASS {
                FOUND.with(|found| found.set(hwnd));
                return BOOL::from(false);
            }
        }
        BOOL::from(true)
    }

    FOUND.with(|found| found.set(HWND(std::ptr::null_mut())));
    // SAFETY：回调签名与 `WNDENUMPROC` 一致；窗口枚举是只读遍历。
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(0));
    }
    let hwnd = FOUND.with(Cell::get);
    (!hwnd.is_invalid()).then_some(hwnd)
}

fn dpi_of(hwnd: HWND) -> u32 {
    // SAFETY：句柄来自本进程窗口枚举；无效句柄返回 0。
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}
