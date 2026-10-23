//! macOS 平台实现：单实例套接字、窗口枚举、几何、全屏、透明度、光标。
//!
//! ⚠️ 本机没有 macOS 可验：所有 `MainThreadMarker::new()` 为 `None` 的路径
//! （非主线程调用，如播放域的轮询任务）会**安全降级为 no-op** 并记日志，
//! Windows 是实测目标。P1 双平台冒烟时逐条校准（见 docs/nsh-shell-plan.md）。

use super::Geometry;
use nsh_platform_macos::objc2::MainThreadMarker;
use nsh_platform_macos::objc2::rc::Retained;
use nsh_platform_macos::objc2_app_kit::{NSApplication, NSWindow};
use nsh_platform_macos::objc2_foundation::{NSPoint, NSSize};

static LISTENER: std::sync::Mutex<Option<std::os::unix::net::UnixListener>> =
    std::sync::Mutex::new(None);

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
    if let Some(marker) = MainThreadMarker::new() {
        NSApplication::sharedApplication(marker).activate();
    }
}

/// 当前窗口**外框**几何（逻辑点）；不在主线程或没有窗口时 `None`。
pub fn window_rect() -> Option<Geometry> {
    let window = main_window()?;
    let frame = window.frame();
    let screen_h = screen_height()?;
    // AppKit 原点在左下：y' = 屏高 − (y + h)，与 Windows 左上原点同口径。
    Some(Geometry {
        x: frame.origin.x as f32,
        y: (screen_h - frame.origin.y - frame.size.height) as f32,
        width: frame.size.width as f32,
        height: frame.size.height as f32,
    })
}

/// 主屏可用高度（换算坐标用；拿不到回落启动高度）。
fn screen_height() -> Option<f64> {
    let marker = MainThreadMarker::new()?;
    let screen = nsh_platform_macos::objc2_app_kit::NSScreen::mainScreen(marker)?;
    Some(screen.frame().size.height)
}

/// 显示器工作区（逻辑点，左/上/右/下；AppKit 已扣菜单栏/Dock）。
pub fn work_area() -> (f32, f32, f32, f32) {
    let frame = (|| {
        let marker = MainThreadMarker::new()?;
        let screen = nsh_platform_macos::objc2_app_kit::NSScreen::mainScreen(marker)?;
        Some(screen.visibleFrame())
    })();
    let Some(frame) = frame else {
        return (0.0, 0.0, 1280.0, 720.0);
    };
    let screen_h = screen_height().unwrap_or(frame.origin.y + frame.size.height);
    (
        frame.origin.x as f32,
        (screen_h - frame.origin.y - frame.size.height) as f32,
        (frame.origin.x + frame.size.width) as f32,
        (screen_h - frame.origin.y) as f32,
    )
}

/// 几何是否与主屏工作区相交（本机单屏口径；多屏校验 P2 校准）。
pub fn on_any_monitor(g: &Geometry) -> bool {
    let (left, top, right, bottom) = work_area();
    g.x < right && left < g.x + g.width && g.y < bottom && top < g.y + g.height
}

/// 应用内容尺寸 + 外框原点（点；内容尺寸语义与 `setContentSize` 一致，
/// 原点按外框换算，标题栏高度按 0 算——与 Tauri 无框 Windows 同口径）。
pub fn apply_content_and_origin(w: f32, h: f32, x: f32, y: f32) {
    let Some(window) = main_window() else {
        return;
    };
    window.setContentSize(NSSize::new(w as f64, h as f64));
    let screen_h = screen_height().unwrap_or(900.0);
    // 左上原点 → AppKit 左下原点；x 是外框左缘。
    window.setFrameOrigin(NSPoint::new(x as f64, screen_h - y as f64 - h as f64));
}

pub fn is_fullscreen() -> bool {
    use nsh_platform_macos::objc2_app_kit::NSWindowStyleMask;
    main_window()
        .map(|window| window.styleMask().contains(NSWindowStyleMask::FullScreen))
        .unwrap_or(false)
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

/// 全屏开关（`toggleFullScreen`；窗口操作按 AppKit 约定须在主线程，
/// 非主线程安全降级为 no-op 并记日志）。
pub fn toggle_fullscreen() {
    let Some(_marker) = MainThreadMarker::new() else {
        log::warn!("[Shell] 全屏切换：不在主线程，忽略（P2 校准）");
        return;
    };
    let Some(window) = main_window() else {
        return;
    };
    // MainThreadMarker 即「已在主线程」的凭据；toggleFullScreen 是安全方法
    //（sender 无用，传 None）。
    window.toggleFullScreen(None);
}

/// 无边框播放的窗口侧：只藏三颗红绿灯 + 透明标题条，**不动 styleMask**
/// （Tauri 壳同款纪律：整体替换会抹掉 FullSizeContentView 位，内容被顶下去）。
pub fn set_frameless(on: bool) {
    use nsh_platform_macos::objc2_app_kit::{NSWindowButton, NSWindowTitleVisibility};
    let Some(window) = main_window() else {
        return;
    };
    window.setTitlebarAppearsTransparent(on);
    window.setTitleVisibility(if on {
        NSWindowTitleVisibility::Hidden
    } else {
        NSWindowTitleVisibility::Visible
    });
    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(btn) = window.standardWindowButton(kind) {
            btn.setHidden(on);
        }
    }
}

/// 窗口透明度（0..1，隐身模式用）。非主线程安全降级为 no-op 并记日志。
pub fn set_window_alpha(alpha: f32) {
    let Some(marker) = MainThreadMarker::new() else {
        log::warn!("[Shell] 窗口透明度：不在主线程，忽略（P2 校准）");
        return;
    };
    let _ = marker;
    let Some(window) = main_window() else {
        return;
    };
    window.setAlphaValue(alpha.clamp(0.0, 1.0) as f64);
}

/// 光标当前位置（窗口内容坐标，点）；不在窗口内或不在主线程时 `None`。
/// （AppKit 坐标原点在左下，换算成左上原点与 Windows 侧同口径。）
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
    let marker = MainThreadMarker::new()?;
    NSApplication::sharedApplication(marker)
        .windows()
        .iter()
        .next()
}

fn socket_path() -> std::path::PathBuf {
    hongguo_store::paths::data_dir().join("nsh-shell.sock")
}
