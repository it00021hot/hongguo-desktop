//! macOS 平台实现：AppKit 落窗/置顶/藏显（底座在 [`crate::macos`]）。
//!
//! 坐标空间是 **AppKit 全局点、左下原点**。`portrait::geom::fit_rect` 吃
//! top-left 语义，所以几何计算前先把窗口/工作区翻到「相对本屏左上、y 向下」，
//! 算完再翻回 AppKit 坐标——几何口径与 Windows 共用同一段纯函数。

use gpui_kit::Window;

use super::{MINI_H, MINI_MARGIN, MINI_W, OuterRect, WindowSnapshot};

/// 壳级轮询：老板键（Carbon 全局热键，回调只置标志，100ms 一拍消费）+
/// 第二次启动的前置请求（单实例 Unix 套接字，见 [`crate::single_instance`]）。
pub(crate) const SHELL_POLLER: bool = true;

/// GPUI prepaint 的舞台原点 → 窗口绝对坐标（系统播放器的视频位用）。
///
/// macOS 的 `AVPlayerLayer` 宿主挂在窗口 contentView 上，其坐标原点与 GPUI
/// prepaint 给的 y 相差一个顶栏高度（既有实现一直按此补偿，本机验证过）；
/// Windows 那边 prepaint 已含顶栏，补偿为 0。差异只在窗口系统，业务层不感知。
pub(crate) fn stage_origin(x: f32, y: f32, chrome_top: f32) -> (f32, f32) {
    (x, y + chrome_top)
}

/// 注册全局快捷键：Carbon `RegisterEventHotKey`（Tauri 的
/// tauri-plugin-global-shortcut 在 macOS 也是这条通路；不申请辅助功能授权）。
pub(crate) fn register_hotkey(mods: u32, vk: u32) -> Result<(), String> {
    crate::macos::register_hotkey(mods, vk)
}

/// 注销全局快捷键。
pub(crate) fn unregister_hotkey() {
    crate::macos::unregister_hotkey();
}

/// 老板键按下标志（读后清）。
pub(crate) fn take_hotkey_fired() -> bool {
    crate::macos::take_hotkey_fired()
}

/// 第二次启动的前置请求（单实例 Unix 套接字，读后复位）。
pub(crate) fn take_focus_request() -> bool {
    crate::single_instance::take_focus_request()
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

/// 竖屏几何：当前窗口 + 可见区（翻成 top-left 语义）→ 目标矩形。
///
/// 返回值是 **gpui `Window::bounds()` 的空间**（逻辑点、左上原点、相对
/// 本屏），与 `mini`/`portrait` 记忆槽位同一口径；AppKit 坐标换算统一由
/// [`apply_frame`] 负责，本函数不再自己翻回去。
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
    Some((x, y, w, h))
}

/// 落窗。入参是 gpui `Window::bounds()` 的空间（逻辑点、左上原点、x/y
/// 相对本屏）——`mini`/`portrait` 的记忆槽位与竖屏 `portrait_geometry`
/// 都是这一份，与 Windows 侧 `apply_frame`（逻辑像素）口径一致。
///
/// AppKit 要的是全局左下原点：x 加回本屏左缘，y 从「顶向下」翻回去。
/// 之前这里把 gpui 坐标直接当 AppKit 坐标落窗，记忆里的 y 是上下颠倒的
/// ——切小窗/竖屏再进就跳到屏幕另一头。
pub(crate) fn apply_frame(window: &Window, cx: &gpui_kit::App, (x, y, w, h): (f64, f64, f64, f64)) {
    let Some((sx, sy, _, sh)) = crate::macos::screen_frame(window) else {
        return;
    };
    set_frame(window, cx, OuterRect::new(x + sx, sy + sh - y - h, w, h));
}

/// 直接把窗口落到给定外框（竖屏铺满档退出还原用）。
pub(crate) fn restore_frame(window: &Window, cx: &gpui_kit::App, rect: OuterRect) {
    set_frame(window, cx, rect);
}

fn set_frame(window: &Window, cx: &gpui_kit::App, rect: OuterRect) {
    // 句柄在借用期内取出，落窗留到借用结束之后：`setFrame_display` 会同步
    // 触发 AppKit 尺寸回调，回调经 `handle.update` 重入 GPUI 的 App。借用期
    // 内落窗会让这次重入直接撞 `RefCell`（日志里的「RefCell already
    // borrowed」），视口尺寸停在旧值——小窗里渲染大屏布局、拖边内容不跟着变。
    let Some(native) = crate::macos::ns_window_of(window) else {
        return;
    };
    window
        .spawn(cx, async move |_| {
            crate::macos::set_frame(&native, (rect.x, rect.y, rect.w, rect.h));
        })
        .detach();
}

/// 窗口矩形（与 [`cursor_pos`] 同一坐标空间：AppKit 全局点、左下原点）。
pub(crate) fn window_rect(window: &Window) -> Option<(f64, f64, f64, f64)> {
    crate::macos::frame(window)
}

/// 窗口记忆槽位的坐标空间 = AppKit 全局外框（含各自显示器原点）。
pub(crate) fn memory_rect(window: &Window) -> Option<OuterRect> {
    current_rect(window)
}

/// 按记忆槽位落窗（AppKit 全局坐标）。
pub(crate) fn memory_restore_frame(window: &Window, cx: &gpui_kit::App, rect: OuterRect) {
    restore_frame(window, cx, rect);
}

/// 记忆槽位校验用的显示器矩形（与 [`memory_rect`] 同一空间）。
pub(crate) fn memory_display_rects(_cx: &gpui_kit::App) -> Vec<(f32, f32, f32, f32)> {
    crate::macos::screens_frames()
        .into_iter()
        .map(|(x, y, w, h)| (x as f32, y as f32, w as f32, h as f32))
        .collect()
}

/// 记忆槽位是不是「窗口创建后再落」：macOS 的 `WindowBounds` 走 gpui 自己的
/// 坐标空间（多屏下丢显示器原点），所以先按默认位置开窗、再用 AppKit 外框
/// 精确恢复。
pub(crate) const RESTORE_AFTER_OPEN: bool = true;

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
