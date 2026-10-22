//! 壳内唯一的平台代码（Windows / macOS 各一份实现）：单实例抢占 + 窗口几何。
//!
//! 为什么还留在壳里：nsh 0.1.1 的 Rust 面**没有**窗口几何 API——
//! `DesktopCommand::SetFrame` 只有 `nsh/desktop` 的 JS/Rust 模块内部
//! （`runtime::desktop::Window::set_frame` 是 `pub(crate)`），`AppView` 只交出
//! `hwnd()` / `view()`。而两壳的小窗语义是「同一个窗口缩成 480×270 落到工作区
//! 右下角」（对齐 Tauri `enter_mini_screen`），只能在原生层做一次。
//!
//! 为了不自己养一套 Win32 声明，Windows 侧**直接用 nsh 转出的 `windows`
//! crate**（`nsh_platform_windows::windows`，0.62，feature 由 nsh 自己开）：
//! 类型、常量、函数签名都跟着上游走，nsh 升级换版本时这里不改。等上游把
//! `nsh/desktop` 的 `setFrame` 开放给 Rust（或 `AppView` 加 set_frame）就删掉
//! 整个 imp 模块。
//!
//! 窗口句柄的取法：`run_app` 内部创建 `AppView` 且不交出所有权，所以按平台
//! 反查——Windows 按 nsh 注册的窗口类名 `NshWindow` + 本进程 pid（类名见
//! `nsh-platform-windows/src/window.rs` 的 `CLASS`）；macOS 取
//! `NSApplication.windows()` 的第一扇。

/// 窗口形态（对齐 Tauri `window_state::Mode` 的三档）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowMode {
    /// 常态：进入小窗/竖屏前的尺寸（无快照时用启动尺寸）。
    Normal,
    /// 小屏播放：480×270 落工作区右下角。
    Mini,
    /// 竖屏播放：高度铺工作区、宽度按视频宽高比收窄（Tauri
    /// `use-portrait-window` 同语义），落工作区右缘。载荷是宽高比（w/h）。
    Portrait(f32),
}

/// 小屏尺寸与落角边距（与 Tauri 壳 `window_cmd.rs` 同参）。
pub const MINI_W: f32 = 480.0;
pub const MINI_H: f32 = 270.0;
pub const MINI_MARGIN: f32 = 16.0;
/// 小屏下的最小尺寸：控制条一行固定内容约 352px，360 是「不换行」的下限。
pub const MINI_MIN: (f32, f32) = (360.0, 180.0);
/// 竖屏条的宽度下限（360 与小屏同下限；窄于它控制条放不下）。
pub const PORTRAIT_MIN_W: f32 = 360.0;
/// 启动尺寸（与 `AppOptions.size` 一致；`Normal` 无快照时回到它）。
pub const START_SIZE: (f32, f32) = (1280.0, 840.0);

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use std::cell::Cell;

    use nsh_platform_windows::windows::Win32::{
        Foundation::{
            ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, POINT, RECT, WAIT_OBJECT_0,
        },
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        System::Threading::{
            CreateEventW, CreateMutexW, GetCurrentProcessId, ResetEvent, SetEvent,
            WaitForSingleObject,
        },
        UI::{
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            WindowsAndMessaging::{
                EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetClientRect, GetCursorPos,
                GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, HWND_NOTOPMOST,
                HWND_TOPMOST, IsWindowVisible, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow,
                WINDOW_EX_STYLE, WINDOW_STYLE,
            },
        },
    };
    use nsh_platform_windows::windows::core::{BOOL, HSTRING};

    /// nsh 注册的顶层窗口类名（`nsh-platform-windows/src/window.rs` 的 `CLASS`）。
    const NSH_CLASS: &str = "NshWindow";

    /// 命名内核对象（`HANDLE` 带裸指针，不是 `Send`；门闩要活到进程退出，
    /// 所以按 `isize` 存进静态量，用时再转回句柄）。
    static SINGLETON: std::sync::Mutex<Option<(isize, isize)>> = std::sync::Mutex::new(None);
    /// 进小屏前的内容区尺寸（`Normal` 还原用）。
    static NORMAL_SIZE: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

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

    /// 切窗口形态。几何一律按**客户区**语义（与 Tauri `set_size` 一致）：
    /// 先把客户区尺寸换算成外框，再 `SetWindowPos`。
    pub fn set_window_mode(mode: WindowMode) {
        let Some(hwnd) = find_window() else {
            log::warn!("[Shell] 切窗口形态：没找到 NshWindow");
            return;
        };
        // 进小窗/竖屏前记一份常态尺寸（重复进不覆盖）。
        if !matches!(mode, WindowMode::Normal) {
            let mut saved = NORMAL_SIZE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if saved.is_none() {
                *saved = client_size();
            }
        }
        let (w, h) = match mode {
            WindowMode::Mini => (MINI_W.max(MINI_MIN.0), MINI_H.max(MINI_MIN.1)),
            WindowMode::Portrait(aspect) => portrait_size(hwnd, aspect),
            WindowMode::Normal => {
                let saved = *NORMAL_SIZE
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let (w, h) = saved.unwrap_or((START_SIZE.0 as i32, START_SIZE.1 as i32));
                (w as f32, h as f32)
            }
        };
        let dpi = dpi_of(hwnd);
        let scale = dpi as f32 / 96.0;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (w * scale).round() as i32,
            bottom: (h * scale).round() as i32,
        };
        // 小屏/竖屏落角用外框尺寸（换算后才能知道窗口真正占多大）。
        let (place_x, place_y) = corner_origin(hwnd, w, h);
        // SAFETY：句柄来自本进程窗口枚举；结构体是本次调用自造的合法缓冲。
        unsafe {
            let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
            let ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
            let _ = AdjustWindowRectExForDpi(&mut rect, style, false, ex, dpi);
            let (outer_w, outer_h) = (rect.right - rect.left, rect.bottom - rect.top);
            let (x, y, flags) = match mode {
                // 小屏/竖屏：精确落角（位置也要改，不能带 SWP_NOMOVE）。
                WindowMode::Mini | WindowMode::Portrait(_) => {
                    (place_x, place_y, SWP_NOZORDER | SWP_NOACTIVATE)
                }
                // 常态：只改尺寸，位置留给用户自己拖过的位置。
                WindowMode::Normal => (0, 0, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE),
            };
            let _ = SetWindowPos(hwnd, None, x, y, outer_w, outer_h, flags);
        }
        log::info!("[Shell] 窗口形态 → {mode:?}（客户区 {w}×{h}，dpi={dpi}）");
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

    /// 竖屏条客户区尺寸：高铺工作区（留边距），宽按宽高比收窄，双向钳位。
    fn portrait_size(hwnd: HWND, aspect: f32) -> (f32, f32) {
        let (work_w, work_h) = work_area(hwnd);
        let margin = MINI_MARGIN;
        let h = (work_h - margin * 2.0).max(MINI_MIN.1);
        let w = (h * aspect.max(0.1)).clamp(PORTRAIT_MIN_W, (work_w - margin * 2.0).max(200.0));
        (w, h)
    }

    /// 窗口所在监视器的工作区尺寸（点）。
    fn work_area(hwnd: HWND) -> (f32, f32) {
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
            return (START_SIZE.0, START_SIZE.1);
        }
        let dpi = dpi_of(hwnd) as f32 / 96.0;
        (
            (info.rcWork.right - info.rcWork.left) as f32 / dpi,
            (info.rcWork.bottom - info.rcWork.top) as f32 / dpi,
        )
    }

    /// 小屏/竖屏落角：监视器工作区右下角减边距（工作区放不下就压到放得下）。
    fn corner_origin(hwnd: HWND, w: f32, h: f32) -> (i32, i32) {
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
            return (0, 0);
        }
        let dpi = dpi_of(hwnd) as f32 / 96.0;
        let (cw, ch) = (w * dpi, h * dpi);
        let margin = MINI_MARGIN * dpi;
        let x = (info.rcWork.right as f32 - cw - margin).max(info.rcWork.left as f32);
        let y = (info.rcWork.bottom as f32 - ch - margin).max(info.rcWork.top as f32);
        (x as i32, y as i32)
    }

    /// 当前客户区尺寸（小屏快照用）。
    pub(super) fn client_size() -> Option<(i32, i32)> {
        let hwnd = find_window()?;
        let mut rect = RECT::default();
        // SAFETY：句柄来自本进程窗口枚举。
        unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
        Some((rect.right - rect.left, rect.bottom - rect.top))
    }

    /// 光标当前位置（窗口内容坐标，点）；不在本进程窗口内时返回 `None`。
    ///
    /// nsh 0.1.1 没有指针移动事件，播放页的「动鼠标唤醒悬浮层」只能靠
    /// 播放域低频轮询这个口子实现（见 `player::mod` 的 150ms 任务）。
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
    fn find_window() -> Option<HWND> {
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
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use nsh_platform_macos::objc2::MainThreadMarker;
    use nsh_platform_macos::objc2::rc::Retained;
    use nsh_platform_macos::objc2_app_kit::{NSApplication, NSWindow};
    use nsh_platform_macos::objc2_foundation::{NSPoint, NSSize};

    pub fn claim_single_instance() -> bool {
        use std::io::Write as _;
        use std::os::unix::net::{UnixListener, UnixStream};

        let path = socket_path();
        // 先试连：连得上 = 已有实例在跑，写一个字节请它前置后退出。
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(b"focus");
            let _ = stream.flush();
            return false;
        }
        // 连不上（没有实例 / 崩溃残留）：清掉陈档再 bind。
        let _ = std::fs::remove_file(&path);
        match UnixListener::bind(&path) {
            Ok(listener) => {
                let _ = listener.set_nonblocking(true);
                *LISTENER
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(listener);
                true
            }
            Err(e) => {
                // bind 失败不拦启动：退回「无单实例」行为，由数据库锁兜底。
                log::warn!("[Shell] 单实例套接字不可用（{path:?}）：{e}");
                true
            }
        }
    }

    /// 首个实例的非阻塞监听器（持有期间套接字路径即单实例锁）。
    static LISTENER: std::sync::Mutex<Option<std::os::unix::net::UnixListener>> =
        std::sync::Mutex::new(None);
    /// 进小屏前的内容区尺寸（`Normal` 还原用）。
    static NORMAL_SIZE: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

    pub fn take_focus_request() -> bool {
        let guard = LISTENER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(listener) = guard.as_ref() else {
            return false;
        };
        let mut requested = false;
        while listener.accept().is_ok() {
            requested = true;
        }
        requested
    }

    pub fn bring_to_front() {
        let Some(window) = main_window() else {
            log::warn!("[Shell] 前置窗口：没找到 NSWindow");
            return;
        };
        window.makeKeyAndOrderFront(None);
        NSApplication::sharedApplication(main_marker()).activate();
    }

    /// 切窗口形态：内容区尺寸语义（与 nsh 自己的 `resize` 脚本动词同一条
    /// 路径——`window.setContentSize`）。
    pub fn set_window_mode(mode: WindowMode) {
        let Some(window) = main_window() else {
            log::warn!("[Shell] 切窗口形态：没找到 NSWindow");
            return;
        };
        let (w, h) = match mode {
            WindowMode::Mini => {
                snapshot_normal_size();
                (MINI_W.max(MINI_MIN.0), MINI_H.max(MINI_MIN.1))
            }
            WindowMode::Portrait(aspect) => {
                snapshot_normal_size();
                portrait_size(aspect)
            }
            WindowMode::Normal => {
                let saved = *NORMAL_SIZE
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let (w, h) = saved.unwrap_or((START_SIZE.0 as i32, START_SIZE.1 as i32));
                (w as f32, h as f32)
            }
        };
        window.setContentSize(NSSize::new(w as f64, h as f64));
        // 竖屏/小屏落右下角（与 Windows 同语义；`setContentSize` 只改尺寸）。
        // AppKit 坐标系原点在左下：y 加 margin 即贴底。
        if let Some(frame) = (|| {
            let marker = MainThreadMarker::new()?;
            let screen = nsh_platform_macos::objc2_app_kit::NSScreen::mainScreen(marker)?;
            Some(screen.frame())
        })() {
            let x = frame.origin.x + frame.size.width - w as f64 - MINI_MARGIN as f64;
            let y = frame.origin.y + MINI_MARGIN as f64;
            window.setFrameOrigin(NSPoint::new(x, y));
        }
        log::info!("[Shell] 窗口形态 → {mode:?}（内容区 {w}×{h}）");
    }

    /// 进小窗/竖屏前记一份常态尺寸（重复进不覆盖）。
    fn snapshot_normal_size() {
        let mut saved = NORMAL_SIZE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if saved.is_none() {
            *saved = client_size();
        }
    }

    /// 竖屏条内容区尺寸：高按主屏可用高度收边距，宽按宽高比收窄（本机
    /// 未实测——`AppKit` 工作区要扣菜单栏/Dock，先用屏幕 frame 收紧边距，
    /// P1 双平台冒烟脚本里加断言再校准）。
    fn portrait_size(aspect: f32) -> (f32, f32) {
        let screen_h = (|| {
            let marker = MainThreadMarker::new()?;
            let screen = nsh_platform_macos::objc2_app_kit::NSScreen::mainScreen(marker)?;
            Some(screen.frame().size.height as f32)
        })()
        .unwrap_or(START_SIZE.1);
        let h = (screen_h - MINI_MARGIN * 2.0).max(MINI_MIN.1);
        let w = (h * aspect.max(0.1)).clamp(PORTRAIT_MIN_W, START_SIZE.0);
        (w, h)
    }

    /// 置顶开关（小窗控制条与播放页共用）：浮层窗口级（`NSFloatingWindowLevel`）。
    pub fn set_always_on_top(top: bool) {
        use nsh_platform_macos::objc2_app_kit::{NSFloatingWindowLevel, NSNormalWindowLevel};
        let Some(window) = main_window() else {
            return;
        };
        window.setLevel(if top {
            NSFloatingWindowLevel
        } else {
            NSNormalWindowLevel
        });
        log::info!("[Shell] 置顶 → {top}");
    }

    /// 当前内容区尺寸（小屏快照用）。
    pub(super) fn client_size() -> Option<(i32, i32)> {
        let size = main_window()?.contentView()?.frame().size;
        Some((size.width as i32, size.height as i32))
    }

    /// 光标当前位置（窗口内容坐标，点）；不在窗口内时返回 `None`。
    ///
    /// AppKit 坐标系原点在左下，换算成左上原点与 Windows 侧同口径。
    /// 轮询方与用途见 `player::mod`（nsh 0.1.1 没有指针移动事件）。
    pub fn cursor_in_window() -> Option<(f32, f32)> {
        use nsh_platform_macos::objc2_app_kit::NSEvent;
        let window = main_window()?;
        let frame = window.frame();
        let mouse = NSEvent::mouseLocation();
        if mouse.x < frame.origin.x
            || mouse.x >= frame.origin.x + frame.size.width
            || mouse.y < frame.origin.y
            || mouse.y >= frame.origin.y + frame.size.height
        {
            return None;
        }
        Some((
            (mouse.x - frame.origin.x) as f32,
            (mouse.y - frame.origin.y) as f32,
        ))
    }

    fn main_window() -> Option<Retained<NSWindow>> {
        NSApplication::sharedApplication(MainThreadMarker::new()?)
            .windows()
            .iter()
            .next()
    }

    fn main_marker() -> MainThreadMarker {
        MainThreadMarker::new().expect("窗口操作只在主线程（run_app 内）调用")
    }

    fn socket_path() -> std::path::PathBuf {
        hongguo_store::paths::data_dir().join("nsh-shell.sock")
    }
}

pub use imp::{
    bring_to_front, claim_single_instance, cursor_in_window, set_always_on_top, set_window_mode,
    take_focus_request,
};

/// 平台无关的形态切换入口（业务层只调这个）。
pub fn enter_mini() {
    set_window_mode(WindowMode::Mini);
}

/// 进竖屏：`aspect` 是当前视频的宽高比（w/h；未知给 9:16 的 0.5625）。
pub fn enter_portrait(aspect: f32) {
    set_window_mode(WindowMode::Portrait(aspect));
}

/// 退出小屏/竖屏，回到进入前的尺寸。
pub fn exit_compact() {
    set_window_mode(WindowMode::Normal);
}

/// 退出小屏（`MiniMode` 的既有入口，语义与 [`exit_compact`] 相同）。
pub fn exit_mini() {
    exit_compact();
}
