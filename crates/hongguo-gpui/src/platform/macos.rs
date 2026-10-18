//! macOS 平台实现：AppKit 落窗/置顶/藏显（底座在 [`crate::macos`]）。
//!
//! 坐标空间是 **AppKit 全局点、左下原点**。`portrait::geom::fit_rect` 吃
//! top-left 语义，所以几何计算前先把窗口/工作区翻到「相对本屏左上、y 向下」，
//! 算完再翻回 AppKit 坐标——几何口径与 Windows 共用同一段纯函数。

use gpui_kit::Window;

use super::{MINI_H, MINI_MARGIN, MINI_W, OuterRect, WindowSnapshot};

/// 壳级轮询（全局快捷键 + 第二次启动前置窗口）：macOS 侧未实现——
/// 全局热键要 Carbon `RegisterEventHotKey` 或 NSEvent 全局监听（后者需要
/// 辅助功能授权），单实例前置要 NSWorkspace 通知；两项都还没落地，这里
/// 如实报不支持，不做「注册成功却从不生效」的假象。
pub(crate) const SHELL_POLLER: bool = false;

/// 注册全局快捷键：本平台未实现。
pub(crate) fn register_hotkey(_mods: u32, _vk: u32) -> Result<(), String> {
    Err(crate::i18n::t("settings.bossKeyPlatformUnsupported"))
}

/// 注销全局快捷键：没有注册过，空操作。
pub(crate) fn unregister_hotkey() {}

/// 老板键按下标志：没有注册实现，恒 false。
pub(crate) fn take_hotkey_fired() -> bool {
    false
}

/// 第二次启动的前置请求：单实例前置未实现，恒 false。
pub(crate) fn take_focus_request() -> bool {
    false
}

/// 最小化态还原：macOS 的 deminiaturize 由 AppKit 自己管，这里不需要
/// 显式还原（`show_foreground` 会把窗口带回前台）。
pub(crate) fn restore_from_minimized(_window: &Window) {}

/// 快照窗口外框与最大化态（最大化 = `isZoomed`）。
pub(crate) fn snapshot(window: &Window) -> Option<WindowSnapshot> {
    let (x, y, w, h) = crate::macos::frame(window)?;
    Some(WindowSnapshot {
        rect: OuterRect::new(x, y, w, h),
        maximized: window.is_maximized(),
    })
}

/// 当前外框（AppKit 点，左下原点）。
pub(crate) fn current_rect(window: &Window) -> Option<OuterRect> {
    snapshot(window).map(|s| s.rect)
}

/// 小屏：缩成 480×270 并落到工作区右下角（AppKit 左下原点：右下角 y 就是
/// 可见区底边 + 边距）。
pub(crate) fn place_mini(window: &Window, cx: &gpui_kit::App) -> Option<()> {
    let (vx, vy, vw, vh) = crate::macos::visible_frame(window)?;
    // 小工作区放不下就压到放得下为止（与 Windows 同一公式）
    let w = MINI_W.min(vw - 2.0 * MINI_MARGIN);
    let h = MINI_H.min(vh - 2.0 * MINI_MARGIN);
    let x = (vx + vw - w - MINI_MARGIN).max(vx);
    let y = (vy + MINI_MARGIN).max(vy);
    set_frame(window, cx, OuterRect::new(x, y, w, h));
    Some(())
}

/// 还原到快照（最大化态先退回普通档再落几何）。
pub(crate) fn restore(window: &Window, cx: &gpui_kit::App, snap: WindowSnapshot) {
    if window.is_maximized() {
        window.zoom_window();
    }
    set_frame(window, cx, snap.rect);
}

/// 置顶（窗口层级：floating ↔ normal）。
pub(crate) fn set_topmost(window: &Window, cx: &gpui_kit::App, on: bool) -> bool {
    // 先探测句柄：拿不到就不翻状态（与 Windows 同口径）
    if crate::macos::ns_window_of(window).is_none() {
        return false;
    }
    window
        .spawn(cx, async move |cx| {
            let _ = cx.update(|window, _| crate::macos::set_topmost(window, on));
        })
        .detach();
    true
}

/// 竖屏几何：当前窗口 + 可见区（翻成 top-left 语义）→ 目标外框（AppKit）。
pub(crate) fn portrait_geometry(
    window: &Window,
    aspect: f64,
    full: bool,
) -> Option<(f64, f64, f64, f64)> {
    use crate::portrait::geom::{FitInput, fit_rect};

    let (sx, sy, _, sh) = crate::macos::screen_frame(window)?;
    let screen_top = sy + sh;
    let (wx, wy, ww, wh) = crate::macos::frame(window)?;
    let (vx, vy, vw, vh) = crate::macos::visible_frame(window)?;
    // AppKit 全局左下原点 → 「相对本屏左上、y 向下」：x 减屏幕左缘，
    // y 用屏幕顶到矩形上缘的距离。多显示器时各屏都在同一全局空间里，
    // 平移+翻转后仍是同一空间，`fit_rect` 的居中/钳制语义不变。
    let flip = |y: f64, h: f64| screen_top - (y + h);
    let work = (vx - sx, flip(vy, vh), vx + vw - sx, flip(vy, 0.0));
    let cur = (wx - sx, flip(wy, wh), ww, wh);
    let (x, y, w, h) = fit_rect(FitInput {
        aspect,
        full,
        cur,
        work,
    });
    // 回 AppKit 坐标：x 平移回屏幕原点，y 从「顶向下」翻回「底向上」
    Some((x + sx, screen_top - y - h, w, h))
}

/// 落窗（AppKit 坐标）。
pub(crate) fn apply_frame(window: &Window, cx: &gpui_kit::App, rect: (f64, f64, f64, f64)) {
    set_frame(window, cx, OuterRect::new(rect.0, rect.1, rect.2, rect.3));
}

/// 直接把窗口落到给定外框（竖屏铺满档退出还原用）。
pub(crate) fn restore_frame(window: &Window, cx: &gpui_kit::App, rect: OuterRect) {
    set_frame(window, cx, rect);
}

fn set_frame(window: &Window, cx: &gpui_kit::App, rect: OuterRect) {
    window
        .spawn(cx, async move |cx| {
            let _ = cx.update(|window, _| {
                crate::macos::set_frame(window, (rect.x, rect.y, rect.w, rect.h));
            });
        })
        .detach();
}

/// 窗口矩形（与 [`cursor_pos`] 同一坐标空间：AppKit 全局点、左下原点）。
pub(crate) fn window_rect(window: &Window) -> Option<(f64, f64, f64, f64)> {
    crate::macos::frame(window)
}

/// 全局光标位置（AppKit 全局点、左下原点）。
pub(crate) fn cursor_pos(_window: &Window) -> Option<(f64, f64)> {
    crate::macos::cursor_pos()
}

pub(crate) fn is_visible(window: &Window) -> bool {
    crate::macos::is_window_visible(window)
}

pub(crate) fn is_minimized(window: &Window) -> bool {
    crate::macos::is_minimized(window)
}

pub(crate) fn hide(window: &Window) {
    crate::macos::hide(window);
}

pub(crate) fn show_no_activate(window: &Window) {
    crate::macos::show_no_activate(window);
}

pub(crate) fn show_foreground(window: &Window) {
    crate::macos::show_foreground(window);
}

/// 藏/显 macOS 红绿灯（无边框播放用）：只 setHidden 三颗圆点，不动 styleMask。
pub(crate) fn set_traffic_lights_hidden(window: &Window, hidden: bool) {
    crate::macos::set_traffic_lights_hidden(window, hidden);
}

/// 当前窗口是否可见（老板键切换判定用）。
pub(crate) fn visibility(window: &Window) -> bool {
    is_visible(window)
}
