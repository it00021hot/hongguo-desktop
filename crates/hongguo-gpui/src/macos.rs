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

use gpui_kit::Window;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSEvent, NSScreen, NSView, NSWindow};
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
pub(crate) fn set_frame(window: &Window, (x, y, w, h): (f64, f64, f64, f64)) {
    let Some(win) = ns_window_of(window) else {
        return;
    };
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
