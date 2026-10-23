//! Win32 窗口原语（user32 / dwmapi 手写 FFI，零新依赖）。
//!
//! gpui 没有暴露的窗口系统能力统一落这里（自 `mini.rs` 迁移），当前调用
//! 方：`mini`（小屏落位/还原/置顶）、无边框窗口的边框线消除与 `home::
//! incognito`（隐身藏显）。光标轮询、按窗口藏显在 gpui 的
//! `PlatformWindow` trait 里只有只读面（`visibility()` 无 setter，全局
//! 光标位置无 API），组件库也不覆盖——这是本项目仅存的 FFI 面，新增窗口
//! 系统能力先查 gpui / gpui-component，别往这里堆。

use gpui_kit::Window;

/// gpui `Window` 的原生 HWND。
pub(crate) fn hwnd_of(window: &Window) -> Option<isize> {
    // Window 有个同名固有方法（返回 AnyWindowHandle），必须全限定调到
    // HasWindowHandle 这个 trait 方法才拿得到原生 HWND
    match raw_window_handle::HasWindowHandle::window_handle(window)
        .ok()?
        .as_raw()
    {
        raw_window_handle::RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}

/// 全局光标位置（物理屏幕坐标）。
pub(crate) fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY：p 是本次调用自造的合法缓冲
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
}

/// 窗口是否最小化。
pub(crate) fn is_iconic(hwnd: isize) -> bool {
    // SAFETY：句柄来自 gpui 的 platform window
    unsafe { IsIconic(hwnd) != 0 }
}

/// 窗口当前是否可见（WS_VISIBLE 位；隐身的「藏」走它判定）。
pub(crate) fn is_window_visible(hwnd: isize) -> bool {
    // SAFETY：句柄来自 gpui 的 platform window
    unsafe { IsWindowVisible(hwnd) != 0 }
}

/// 显示窗口并夺焦点（关闭隐身这类「用户要窗口回来」的路径用）。
pub(crate) fn show_foreground(hwnd: isize) {
    show_window(hwnd, SW_SHOW);
    // SAFETY：句柄来自 gpui 的 platform window
    unsafe {
        SetForegroundWindow(hwnd);
    }
}

/// 显示窗口但不夺焦点（隐身里「鼠标回来了」的重现：不打断用户手上的事）。
pub(crate) fn show_no_activate(hwnd: isize) {
    show_window(hwnd, SW_SHOWNA);
}

/// 隐藏窗口。
pub(crate) fn hide(hwnd: isize) {
    show_window(hwnd, SW_HIDE);
}

/// 去掉窗口四周的 1px 系统边框线（Win11 22H2+；失败静默——旧系统没有
/// 这个属性，保留默认外观）。
///
/// gpui 在 Windows 的窗口带 `WS_THICKFRAME`（可缩放边 + 贴靠），DWM 给它
/// 描一圈边框线；Tauri `decorations:false` 是纯 `WS_POPUP` 所以没有这条
/// 线。组件库的 TitleBar 只自绘标题栏内容，边框线不归它管——这里按
/// `DWMWA_BORDER_COLOR = NONE` 关掉线，缩放/贴靠/最大化行为全部保留。
pub(crate) fn hide_border(window: &Window) {
    let Some(hwnd) = hwnd_of(window) else {
        return;
    };
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    let color = DWMWA_COLOR_NONE;
    // SAFETY：句柄来自 gpui 的 platform window，value 是本次调用自造值
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            std::ptr::from_ref(&color).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    if hr != 0 {
        log::debug!("[Win32] 去边框线失败（hr={hr}）——系统不支持 DWMWA_BORDER_COLOR，保留默认");
    }
}

/// Win32 调用薄封装：逻辑像素在这里换算成物理像素（窗口 API 只认物理）。
pub(crate) fn set_window_pos(
    hwnd: isize,
    after: isize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    flags: u32,
) -> bool {
    // SAFETY：句柄来自 gpui 的 platform window，参数全是本次调用自造值
    unsafe {
        SetWindowPos(
            hwnd,
            after,
            x.round() as i32,
            y.round() as i32,
            w.round() as i32,
            h.round() as i32,
            flags,
        ) != 0
    }
}

/// 物理像素直通版（还原快照用，不做任何换算）。
pub(crate) fn set_window_pos_raw(
    hwnd: isize,
    after: isize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    flags: u32,
) {
    // SAFETY：同上
    unsafe {
        SetWindowPos(hwnd, after, x, y, w, h, flags);
    }
}

/// 外框矩形（物理像素 left/top/right/bottom）。
pub(crate) fn window_rect(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY：rc 是本次调用自造的合法缓冲
    let ok = unsafe { GetWindowRect(hwnd, &mut rc) };
    (ok != 0).then_some((rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top))
}

/// 边框偏移（物理像素）：`(左, 顶, 右侧宽, 底部高)`——外框−客户区的四向
/// 差。SetWindowPos 设外框，而小窗目标 480×270 是 Tauri set_size 的
/// 「内容尺寸」语义，落位/定尺寸都要补上这个差（只补总差会把底边推进
/// 任务栏，实测外框底到了 1415 > 工作区底 1376）。
pub(crate) fn border_offset(hwnd: isize) -> (i32, i32, i32, i32) {
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut outer = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY：三个结构都是本次调用自造的合法缓冲；MapWindowPoints 把
    // 客户区 (0,0) 映射到屏幕坐标，得到客户区原点在外框内的偏移
    let ok = unsafe { GetClientRect(hwnd, &mut client) } != 0
        && unsafe { GetWindowRect(hwnd, &mut outer) } != 0
        && unsafe { MapWindowPoints(hwnd, 0, &mut origin, 1) } != 0;
    if !ok {
        return (0, 0, 0, 0);
    }
    let (off_l, off_t) = (origin.x - outer.left, origin.y - outer.top);
    (
        off_l,
        off_t,
        (outer.right - outer.left) - (client.right - client.left) - off_l,
        (outer.bottom - outer.top) - (client.bottom - client.top) - off_t,
    )
}

pub(crate) fn show_window(hwnd: isize, cmd: i32) {
    // SAFETY：同上，句柄来自 gpui platform window
    unsafe {
        ShowWindow(hwnd, cmd);
    }
}

/// 当前显示器的工作区（任务栏除外；逻辑像素 left/top/right/bottom）。
pub(crate) fn work_area(hwnd: isize, scale: f64) -> Option<(f64, f64, f64, f64)> {
    let mut info = MONITORINFO {
        cb_size: std::mem::size_of::<MONITORINFO>() as u32,
        rc_monitor: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rc_work: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dw_flags: 0,
    };
    // SAFETY：info 是本次调用自造的合法缓冲，cb_size 已按约定填好
    let ok =
        unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) };
    if ok == 0 {
        return None;
    }
    let rc = info.rc_work;
    Some((
        rc.left as f64 / scale,
        rc.top as f64 / scale,
        rc.right as f64 / scale,
        rc.bottom as f64 / scale,
    ))
}

pub(crate) const HWND_TOP: isize = 0;
pub(crate) const HWND_TOPMOST: isize = -1;
pub(crate) const HWND_NOTOPMOST: isize = -2;
pub(crate) const SWP_NOSIZE: u32 = 0x0001;
pub(crate) const SWP_NOMOVE: u32 = 0x0002;
pub(crate) const SWP_NOACTIVATE: u32 = 0x0010;
pub(crate) const SW_HIDE: i32 = 0;
/// `SW_MAXIMIZE` / `SW_RESTORE`：双击顶栏切最大化/还原
pub(crate) const SW_MAXIMIZE: i32 = 3;
pub(crate) const SW_RESTORE: i32 = 9;
pub(crate) const SW_SHOW: i32 = 5;
pub(crate) const SW_SHOWNA: i32 = 8;
const MONITOR_DEFAULTTONEAREST: u32 = 2;

/// 已注册的全局快捷键（老板键）状态：注册到本进程自建的**消息窗口**上。
///
/// 为什么自建窗口而不是挂 gpui 的窗口：gpui 的窗口过程不对外暴露（没有
/// 原始消息钩子），WM_HOTKEY 会被它当未知消息丢掉；子类化 gpui 的
/// HWND 又要动它的窗口过程，风险不对等。自建一个 message-only 窗口
/// （父 = HWND_MESSAGE）接 WM_HOTKEY，与 gpui 完全隔离——同一线程，
/// gpui 的消息循环会照常把它派发到我们的窗口过程。
static HOTKEY: std::sync::Mutex<Option<HotkeyReg>> = std::sync::Mutex::new(None);
/// 老板键按下标志（窗口过程置位，壳内轮询消费）。窗口过程跑在主线程的
/// 消息循环里，拿不到 GPUI 上下文，只能这样把事件递进去。
static HOTKEY_FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct HotkeyReg {
    hwnd: isize,
    id: i32,
}

/// 注册全局快捷键（[`crate::boss_key::accel`] 算好的修饰位 + 虚拟键码）。
/// 返回 Err = 注册失败（键被别的程序占用等，Tauri 侧同语义）。重复注册先
/// 注销旧键，失败时旧键会被回落注回。
pub(crate) fn register_hotkey(mods: u32, vk: u32) -> Result<(), String> {
    let hwnd = message_window()?;
    let id = 1;
    let mut cur = HOTKEY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(old) = cur.as_ref() {
        // SAFETY：id 是本进程注册的快捷键 id；hwnd 是自建消息窗口
        unsafe { UnregisterHotKey(old.hwnd, old.id) };
    }
    // SAFETY：mods/vk 由调用方从 accelerator 解析而来；hwnd 有效
    let ok = unsafe { RegisterHotKey(hwnd, id, mods, vk) };
    if ok == 0 {
        *cur = None;
        return Err(crate::i18n::t("settings.bossKeyRegisterFailed"));
    }
    *cur = Some(HotkeyReg { hwnd, id });
    Ok(())
}

/// 注销全局快捷键（幂等；未注册时是空操作）。
pub(crate) fn unregister_hotkey() {
    let mut cur = HOTKEY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(old) = cur.take() {
        // SAFETY：与注册成对，id 是本进程注册的
        unsafe { UnregisterHotKey(old.hwnd, old.id) };
    }
}

/// 老板键是否被按下过（读后清；壳内轮询任务消费）。
pub(crate) fn take_hotkey_fired() -> bool {
    HOTKEY_FIRED.swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// 合成一次组合键（取证探针用：验「注册→系统认下→WM_HOTKEY→藏窗」整条链，
/// 不是模拟业务行为）。按下与抬起成对发出，任何中途返回都不会留下按住的键。
#[cfg(feature = "screenshot")]
pub(crate) fn send_key_chord(mods: u32, vk: u32) {
    use crate::boss_key::accel::{MOD_ALT, MOD_CONTROL, MOD_SHIFT};
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const VK_MENU: u8 = 0x12;
    const VK_CONTROL: u8 = 0x11;
    const VK_SHIFT: u8 = 0x10;
    let mut held: Vec<u8> = Vec::new();
    // SAFETY：keybd_event 参数是普通整数；按下与抬起一一对应
    unsafe {
        if mods & MOD_CONTROL != 0 {
            keybd_event(VK_CONTROL, 0, 0, 0);
            held.push(VK_CONTROL);
        }
        if mods & MOD_ALT != 0 {
            keybd_event(VK_MENU, 0, 0, 0);
            held.push(VK_MENU);
        }
        if mods & MOD_SHIFT != 0 {
            keybd_event(VK_SHIFT, 0, 0, 0);
            held.push(VK_SHIFT);
        }
        keybd_event(vk as u8, 0, 0, 0);
        keybd_event(vk as u8, 0, KEYEVENTF_KEYUP, 0);
        for key in held.into_iter().rev() {
            keybd_event(key, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

/// 投递一次 WM_CLOSE（取证探针用：验退出确认拦截是否真的吃住窗口关闭请求，
/// 不是模拟业务行为）。
#[cfg(feature = "screenshot")]
pub(crate) fn post_close(hwnd: isize) {
    // SAFETY：句柄来自 gpui 的 platform window；消息是常量
    unsafe {
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

/// 起拖窗口（自绘顶栏空白带按住拖动）。
///
/// `ReleaseCapture + WM_NCLBUTTONDOWN(HTCAPTION)` 是让窗口进系统移动循环的
/// 标准手法，拖拽手感/贴靠/多显示器全跟系统走。为什么不用 gpui 的
/// `Window::start_window_move()`：那是平台 trait 的**默认空实现**，
/// gpui-pre-windows 没有覆写（macOS 那头才接了 performWindowDragWithEvent）
/// ——Windows 上自绘拖拽只能自己走这一步。
pub(crate) fn start_window_move(hwnd: isize) {
    // SAFETY：句柄来自 gpui 的 platform window；WM_NCLBUTTONDOWN 的 wparam
    // 是命中码（HTCAPTION），lparam 客户区坐标 0 = 用当前光标位置
    unsafe {
        ReleaseCapture();
        SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
    }
}

/// 双击顶栏空白带：最大化 ↔ 还原（`IsZoomed` 判当前态）。
pub(crate) fn toggle_maximize(hwnd: isize) {
    // SAFETY：句柄来自 gpui 的 platform window
    unsafe {
        let cmd = if IsZoomed(hwnd) != 0 {
            SW_RESTORE
        } else {
            SW_MAXIMIZE
        };
        ShowWindow(hwnd, cmd);
    }
}

/// 单实例门闩：命名互斥体表征「本进程是唯一实例」，命名事件承载
/// 「第二次启动请求把窗口带到前台」。
///
/// gpui 没有单实例设施，Tauri 侧走 `tauri-plugin-single-instance`——那边第二
/// 次启动会让插件回调前置已有窗口，而这里是「第二个进程置位事件、首个实例的
/// 轮询消费」：同一语义，零新依赖。**必须早于数据库打开**（上一个实例可能正
/// 持库锁：隐身藏窗/小屏形态都在跑）。
static SINGLETON: std::sync::Mutex<Option<(isize, isize)>> = std::sync::Mutex::new(None);

/// 尝试成为唯一实例。首个实例返回 `Some(事件句柄)`（供轮询前置请求）；
/// 已有实例在跑则置位它的事件、返回 `None`（调用方安静退出）。
pub(crate) fn claim_single_instance() -> Option<isize> {
    let mutex_name = wide("HongguoGpui.SingleInstance");
    let event_name = wide("HongguoGpui.SingleInstance.Focus");
    // SAFETY：两个名字都是本次调用自造的 NUL 结尾 UTF-16；句柄配对关闭
    unsafe {
        let mutex = CreateMutexW(0, 1, mutex_name.as_ptr());
        if mutex == 0 {
            log::warn!("[Shell] 单实例互斥体创建失败，按独占启动继续");
            return None;
        }
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        let event = CreateEventW(0, 1, 0, event_name.as_ptr());
        log::info!("[Shell] 单实例：mutex={mutex} event={event} already={already}");
        if already {
            if event != 0 {
                let ok = SetEvent(event);
                log::info!("[Shell] 单实例：置位前置事件 ok={ok}");
                CloseHandle(event);
            }
            CloseHandle(mutex);
            return None;
        }
        *SINGLETON
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((mutex, event));
        (event != 0).then_some(event)
    }
}

/// 首个实例的轮询消费：第二次启动是否请求前置窗口（读后复位）。
/// 事件没拿到（创建失败）时恒 false——不蹦「永远前置不了」的空转。
pub(crate) fn take_focus_request(event: isize) -> bool {
    if event == 0 {
        return false;
    }
    // SAFETY：event 是本进程 create/claim 出来的事件句柄
    unsafe {
        if WaitForSingleObject(event, 0) != WAIT_OBJECT_0 {
            return false;
        }
        ResetEvent(event);
    }
    true
}

/// 懒建消息窗口（幂等）。类注册一次，窗口句柄缓存在静态里。
fn message_window() -> Result<isize, String> {
    static HWND: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    if let Some(hwnd) = HWND.get() {
        return Ok(*hwnd);
    }
    // SAFETY：类名/窗口过程是进程内常量；hinstance 取自身模块（允许 NULL）
    let hwnd = unsafe {
        let class_name = wide("HongguoGpuiHotkey");
        let wc = WNDCLASSW {
            style: 0,
            lpfn_wnd_proc: Some(hotkey_wnd_proc),
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: 0,
            h_icon: 0,
            h_cursor: 0,
            hbr_background: 0,
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: class_name.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            return Err(crate::i18n::t("settings.bossKeyMessageWindowClassFailed"));
        }
        // HWND_MESSAGE = -3：message-only 窗口，永不可见、不进任务栏
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            0,
            0,
            std::ptr::null(),
        );
        if hwnd == 0 {
            return Err(crate::i18n::t("settings.bossKeyMessageWindowCreateFailed"));
        }
        hwnd
    };
    let _ = HWND.set(hwnd);
    Ok(hwnd)
}

/// 消息窗口过程：只认 WM_HOTKEY（置位给壳内轮询），其余交回系统默认。
unsafe extern "system" fn hotkey_wnd_proc(
    hwnd: isize,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if msg == WM_HOTKEY {
        HOTKEY_FIRED.store(true, std::sync::atomic::Ordering::SeqCst);
        return 0;
    }
    // SAFETY：转发给系统默认处理（self/msg/wparam/lparam 原样透传）
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// UTF-16 + NUL 结尾（Win32 W 系 API 的字符串形态）。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const HWND_MESSAGE: isize = -3;
const WM_HOTKEY: u32 = 0x0312;
/// `WM_NCLBUTTONDOWN`：非客户区左键按下（配 HTCAPTION 起拖窗口）
const WM_NCLBUTTONDOWN: u32 = 0x00A1;
/// `HTCAPTION`：命中码——标题栏（拖动窗口）
const HTCAPTION: i32 = 2;
/// `GetLastError`：命名对象已存在（第二次启动）。
const ERROR_ALREADY_EXISTS: u32 = 183;
/// `WaitForSingleObject`：对象已置位。
const WAIT_OBJECT_0: u32 = 0;
#[cfg(feature = "screenshot")]
const WM_CLOSE: u32 = 0x0010;

type WndProc = unsafe extern "system" fn(isize, u32, usize, isize) -> isize;

#[repr(C)]
#[expect(
    clippy::upper_case_acronyms,
    reason = "Win32 API 结构体名保真（WNDCLASSW 契约）"
)]
struct WNDCLASSW {
    style: u32,
    lpfn_wnd_proc: Option<WndProc>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: isize,
    h_icon: isize,
    h_cursor: isize,
    hbr_background: isize,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
}

// SAFETY：全部是 user32 的标准函数签名（Windows 契约保真）。
unsafe extern "system" {
    unsafe fn RegisterClassW(class: *const WNDCLASSW) -> u16;
    unsafe fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        param: *const core::ffi::c_void,
    ) -> isize;
    unsafe fn DefWindowProcW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    unsafe fn RegisterHotKey(hwnd: isize, id: i32, mods: u32, vk: u32) -> i32;
    unsafe fn UnregisterHotKey(hwnd: isize, id: i32) -> i32;
    unsafe fn CreateMutexW(attrs: isize, initial_owner: i32, name: *const u16) -> isize;
    unsafe fn CreateEventW(
        attrs: isize,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> isize;
    unsafe fn SetEvent(event: isize) -> i32;
    unsafe fn ResetEvent(event: isize) -> i32;
    unsafe fn WaitForSingleObject(handle: isize, millis: u32) -> u32;
    unsafe fn CloseHandle(handle: isize) -> i32;
    unsafe fn GetLastError() -> u32;
    unsafe fn ReleaseCapture() -> i32;
    unsafe fn SendMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    #[cfg(feature = "screenshot")]
    unsafe fn keybd_event(bvk: u8, scan: u8, flags: u32, extra: usize);
    #[cfg(feature = "screenshot")]
    unsafe fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
}

// SAFETY：全部是 user32 / dwmapi 的标准函数签名（Windows 契约保真）。
// user32 由既有链接链带入（mini 路径裸跑多年）；dwmapi 本壳首次用到，
// 显式挂导入库。
unsafe extern "system" {
    unsafe fn SetWindowPos(
        hwnd: isize,
        after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    unsafe fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    unsafe fn MonitorFromWindow(hwnd: isize, flags: u32) -> isize;
    unsafe fn GetMonitorInfoW(monitor: isize, info: *mut MONITORINFO) -> i32;
    unsafe fn GetWindowRect(hwnd: isize, rect: *mut RECT) -> i32;
    unsafe fn GetClientRect(hwnd: isize, rect: *mut RECT) -> i32;
    unsafe fn MapWindowPoints(from: isize, to: isize, point: *mut POINT, count: i32) -> i32;
    unsafe fn GetCursorPos(point: *mut POINT) -> i32;
    unsafe fn IsIconic(hwnd: isize) -> i32;
    unsafe fn IsZoomed(hwnd: isize) -> i32;
    unsafe fn IsWindowVisible(hwnd: isize) -> i32;
    unsafe fn SetForegroundWindow(hwnd: isize) -> i32;
}

#[repr(C)]
#[derive(Clone, Copy)]
#[expect(
    clippy::upper_case_acronyms,
    reason = "Win32 API 结构体名保真（POINT 契约）"
)]
struct POINT {
    x: i32,
    y: i32,
}

#[repr(C)]
#[expect(
    clippy::upper_case_acronyms,
    reason = "Win32 API 结构体名保真（RECTL 契约）"
)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[expect(
    clippy::upper_case_acronyms,
    reason = "Win32 API 结构体名保真（GetMonitorInfoW 契约）"
)]
struct MONITORINFO {
    cb_size: u32,
    rc_monitor: RECT,
    rc_work: RECT,
    dw_flags: u32,
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    unsafe fn DwmSetWindowAttribute(
        hwnd: isize,
        attr: u32,
        value: *const core::ffi::c_void,
        size: u32,
    ) -> i32;
}
