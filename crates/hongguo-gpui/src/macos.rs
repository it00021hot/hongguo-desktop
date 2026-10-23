//! macOS 窗口原语（objc2，零新外部依赖——gpui-pre-macos 的依赖树里已有
//! 同版本的 objc2 / objc2-app-kit）。
//!
//! gpui 没有暴露的窗口系统能力统一落这里，与 [`crate::win32`] 同一定位：
//! 小屏落位/还原、置顶、竖屏几何、隐身藏显。窗口句柄经 `raw-window-handle`
//! 的 AppKit 面取（gpui `Window` 实现了 `HasWindowHandle`），拿到 `NSView`
//! 后转 `NSWindow`。
//!
//! 坐标约定：AppKit 的全局坐标是**左下原点**，本文件对外原样使用；
//! 需要 top-left 语义的调用方（竖屏几何的 `fit_rect`）自己做翻转——两套
//! 坐标只在 [`crate::portrait`] 的 macOS 分支里交汇。
//!
//! 线程纪律：这些方法都必须在主线程调用（gpui 的事件/更新回调本来就在主
//! 线程）；`MainThreadMarker` 只在需要它的入口（`NSScreen` 静态查询）取。

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use gpui_kit::Window;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSEvent, NSScreen, NSView, NSWindow, NSWindowButton};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// gpui 窗口背后的 `NSWindow`。
///
/// GPUI 的 `HasWindowHandle` 返回的是 `NSView`（AppKit 的视图句柄语义），
/// `NSView::window()` 才是宿主窗口。
pub(crate) fn ns_window_of(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?.as_raw();
    let RawWindowHandle::AppKit(handle) = handle else {
        return None;
    };
    // SAFETY：gpui-pre-macos 的 AppKitWindowHandle.ns_view 就是它自己的
    // GPUIView；窗口生命周期由 gpui 持有，本函数只在主线程调用。
    let view = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };
    view.window()
}

/// 窗口外框矩形 `(x, y, w, h)`（AppKit 全局坐标，点）。
pub(crate) fn frame(window: &Window) -> Option<(f64, f64, f64, f64)> {
    let win = ns_window_of(window)?;
    let rect = win.frame();
    Some((
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    ))
}

/// 窗口所在屏幕（窗口还没上屏时回落主屏）。
fn screen_of(window: &Window) -> Option<Retained<NSScreen>> {
    let win = ns_window_of(window)?;
    win.screen()
        .or_else(|| NSScreen::mainScreen(objc2::MainThreadMarker::new()?))
}

/// 全部显示器的外框（AppKit 全局坐标，点；左下原点，含各自原点）。
///
/// 窗口几何记忆用这份而不是 gpui 的 `display.bounds()`：gpui-pre-macos 的
/// display bounds 把显示器原点丢成 (0,0)，多屏下所有屏幕叠在原点、窗口
/// bounds 又是「相对本屏」的——跨屏恢复必然错位。
pub(crate) fn screens_frames() -> Vec<(f64, f64, f64, f64)> {
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return Vec::new();
    };
    NSScreen::screens(mtm)
        .iter()
        .map(|screen| {
            let rect = screen.frame();
            (
                rect.origin.x,
                rect.origin.y,
                rect.size.width,
                rect.size.height,
            )
        })
        .collect()
}

/// 窗口所在屏幕的可见区域（AppKit 全局坐标，点；已扣菜单栏与 Dock）。
pub(crate) fn visible_frame(window: &Window) -> Option<(f64, f64, f64, f64)> {
    let rect = screen_of(window)?.visibleFrame();
    Some((
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    ))
}

/// 窗口所在屏幕的完整区域（AppKit 全局坐标，点）——竖屏几何的 y 翻转基准。
pub(crate) fn screen_frame(window: &Window) -> Option<(f64, f64, f64, f64)> {
    let rect = screen_of(window)?.frame();
    Some((
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    ))
}

/// 落窗（AppKit 坐标；立即重绘）。
///
/// 只接受已经取出的 `NSWindow`：`setFrame_display` 会同步触发 AppKit 的
/// 尺寸回调，回调要重入 GPUI 的 `App` 才能在下一帧拿到新视口。若在
/// `App` 借用期内落窗，这次重入会被 `RefCell` 拒绝，视口尺寸从此停在旧值
///（小窗里渲染大屏布局、拖边看不到内容跟着变）。
pub(crate) fn set_frame(win: &NSWindow, (x, y, w, h): (f64, f64, f64, f64)) {
    let rect = objc2_foundation::NSRect {
        origin: objc2_foundation::NSPoint { x, y },
        size: objc2_foundation::NSSize {
            width: w,
            height: h,
        },
    };
    win.setFrame_display(rect, true);
}

/// 钉住/取消钉住（Tauri `set_always_on_top` 同语义）。
/// 返回 false = 拿不到窗口（调用方不翻状态）。
pub(crate) fn set_topmost(window: &Window, on: bool) -> bool {
    let Some(win) = ns_window_of(window) else {
        return false;
    };
    let level = if on {
        objc2_app_kit::NSFloatingWindowLevel
    } else {
        objc2_app_kit::NSNormalWindowLevel
    };
    win.setLevel(level);
    true
}

/// 藏/显原生红绿灯的三颗圆点（无边框播放模式用；Tauri 壳 `window_cmd.rs`
/// 同口径——只 setHidden，**不动 styleMask**：整体替换会抹掉 Overlay 全出血
/// 依赖的 `FullSizeContentView` 位，窗口尺寸语义跟着漂）。
pub(crate) fn set_traffic_lights_hidden(window: &Window, hidden: bool) {
    let Some(win) = ns_window_of(window) else {
        return;
    };
    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(btn) = win.standardWindowButton(kind) {
            btn.setHidden(hidden);
        }
    }
}

/// 全局光标位置（AppKit 全局坐标，点；左下原点，与 [`frame`] 同空间）。
pub(crate) fn cursor_pos() -> Option<(f64, f64)> {
    let p = NSEvent::mouseLocation();
    Some((p.x, p.y))
}

/// 窗口是否可见。
pub(crate) fn is_window_visible(window: &Window) -> bool {
    ns_window_of(window).is_some_and(|win| win.isVisible())
}

/// 窗口是否最小化。
pub(crate) fn is_minimized(window: &Window) -> bool {
    ns_window_of(window).is_some_and(|win| win.isMiniaturized())
}

/// 隐藏窗口（隐身模式）。
pub(crate) fn hide(window: &Window) {
    if let Some(win) = ns_window_of(window) {
        win.orderOut(None);
    }
}

/// 显示窗口但不夺焦点（隐身里「鼠标回来」的重现）。
pub(crate) fn show_no_activate(window: &Window) {
    if let Some(win) = ns_window_of(window) {
        win.orderFrontRegardless();
    }
}

/// 显示窗口并夺焦点（关闭隐身/第二次启动前置）。
pub(crate) fn show_foreground(window: &Window) {
    if let Some(win) = ns_window_of(window) {
        win.orderFrontRegardless();
    }
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).activate();
    }
}

// —— 全局快捷键（Carbon RegisterEventHotKey）——
//
// Tauri 用 tauri-plugin-global-shortcut（macOS 侧也是 Carbon），gpui 没有
// 全局快捷键面，这里按 `win32.rs` 的同一分工手写 FFI：注册/注销在主线程，
// 回调只置一个原子标志，壳内 100ms 轮询消费（窗口过程/事件回调里拿不到
// GPUI 上下文，不能直接动视图）。
//
// Carbon 虽被标为 deprecated，但 RegisterEventHotKey 仍是系统级热键的
// 稳定通路（不申请辅助功能授权，也不需要 Input Monitoring），与 Tauri
// 的实现同源。

#[repr(C)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn RegisterEventHotKey(
        hot_key_code: u32,
        hot_key_modifiers: u32,
        hot_key_id: EventHotKeyID,
        target: *mut c_void,
        options: u32,
        out_ref: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(reference: *mut c_void) -> i32;
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
        num_types: usize,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut *mut c_void,
    ) -> i32;
}

/// `kEventClassKeyboard`（'keyb'）与 `kEventHotKeyPressed`。
const K_EVENT_CLASS_KEYBOARD: u32 = 0x6B65_7962;
const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
/// 热键签名（'HGBK'），只在本进程内区分，与系统其它热键不冲突。
const HOTKEY_SIGNATURE: u32 = 0x4847_424B;

static HOTKEY_FIRED: AtomicBool = AtomicBool::new(false);
static HOTKEY_REF: Mutex<Option<usize>> = Mutex::new(None);
static HOTKEY_HANDLER: Once = Once::new();
static HOTKEY_HANDLER_STATUS: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" fn hotkey_pressed(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> i32 {
    HOTKEY_FIRED.store(true, Ordering::SeqCst);
    0 // noErr
}

fn install_hotkey_handler() -> Result<(), String> {
    HOTKEY_HANDLER.call_once(|| {
        let spec = EventTypeSpec {
            event_class: K_EVENT_CLASS_KEYBOARD,
            event_kind: K_EVENT_HOT_KEY_PRESSED,
        };
        let mut handler_ref: *mut c_void = std::ptr::null_mut();
        // SAFETY：spec/handler_ref 都是本函数的栈上值；application target 由系统持有
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                hotkey_pressed,
                1,
                &spec,
                std::ptr::null_mut(),
                &mut handler_ref,
            )
        };
        HOTKEY_HANDLER_STATUS.store(status, Ordering::SeqCst);
        if status != 0 {
            log::warn!("[BossKey] Carbon 事件处理器安装失败：status={status}");
        }
    });
    if HOTKEY_HANDLER_STATUS.load(Ordering::SeqCst) == 0 {
        Ok(())
    } else {
        Err(crate::i18n::t("settings.bossKeyRegisterFailed"))
    }
}

/// 注册全局快捷键（修饰位是 Win32 `MOD_*`，键码是 Win32 虚拟键码）。
/// 返回 Err = 本平台没有该物理键 / 系统拒绝（已被别的程序占用）。
pub(crate) fn register_hotkey(mods: u32, vk: u32) -> Result<(), String> {
    use crate::boss_key::accel::{carbon_mods, mac_keycode};

    let Some(code) = mac_keycode(vk) else {
        return Err(crate::i18n::t("settings.bossKeyRegisterFailed"));
    };
    install_hotkey_handler()?;
    unregister_hotkey();

    let mut reference: *mut c_void = std::ptr::null_mut();
    // SAFETY：参数来自 accelerator 解析；reference 是本函数栈上值，成功后交给静态槽位持有
    let status = unsafe {
        RegisterEventHotKey(
            code,
            carbon_mods(mods),
            EventHotKeyID {
                signature: HOTKEY_SIGNATURE,
                id: 1,
            },
            GetApplicationEventTarget(),
            0,
            &mut reference,
        )
    };
    if status != 0 || reference.is_null() {
        log::warn!("[BossKey] Carbon 热键注册失败：status={status}");
        return Err(crate::i18n::t("settings.bossKeyRegisterFailed"));
    }
    *HOTKEY_REF
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reference as usize);
    log::info!("[BossKey] 全局热键已注册（Carbon keycode={code}）");
    Ok(())
}

/// 注销全局快捷键（没有注册过是空操作）。
pub(crate) fn unregister_hotkey() {
    let taken = HOTKEY_REF
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(reference) = taken {
        // SAFETY：句柄是本进程注册成功时存下的，注销一次
        unsafe { UnregisterEventHotKey(reference as *mut c_void) };
    }
}

/// 老板键是否被按下过（读后清；壳内轮询任务消费）。
pub(crate) fn take_hotkey_fired() -> bool {
    HOTKEY_FIRED.swap(false, Ordering::SeqCst)
}
