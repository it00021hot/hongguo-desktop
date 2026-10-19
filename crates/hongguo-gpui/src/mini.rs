//! 小屏播放的窗口几何（Tauri `enter_mini_screen`/`exit_mini_screen` 对齐）：
//! **同一个窗口**缩成 480×270、落到工作区右下角，退出按快照原样还原——
//! 不另开窗口、不藏窗、不暂停，播放零中断（「独立置顶小窗」是 Tauri 侧
//! 已否决的杜撰设计，见 hongguo-tauri `app_cmd.rs` 的注释）。
//!
//! 平台差异全部在 [`crate::platform`]：落位公式、坐标空间、边框换算各自
//! 实现在 windows/macos 子树里，本模块只做业务编排（快照 → 落位 → 还原）。

use gpui_kit::Window;

pub(crate) use crate::platform::WindowSnapshot;

/// 进入小屏：快照 → 落位。上次小窗几何有效（不小于小屏恢复下限且落在
/// 任一显示器内）就原样回去——三种窗态各记各的槽位，切形态互不覆盖；
/// 没有记忆（首次进小屏）才回落 480×270 工作区右下角默认落角。几何不可
/// 得时返回 `None`，调用方保持原状（与 Tauri 幂等注释同口径：已在小屏时
/// 调用方不得再进来，否则快照会被小屏几何覆盖）。
pub(crate) fn enter(window: &mut Window, cx: &gpui_kit::App) -> Option<WindowSnapshot> {
    let saved = crate::platform::snapshot(window)?;
    let prefs = crate::ui_prefs::load();
    match crate::window_restore::restore_rect(
        prefs.mini_x,
        prefs.mini_y,
        prefs.mini_w,
        prefs.mini_h,
        &crate::platform::display_rects(cx),
        crate::window_restore::MINI_RESTORE_W,
        crate::window_restore::MINI_RESTORE_H,
    ) {
        Some((x, y, w, h)) => {
            crate::platform::apply_frame(
                window,
                cx,
                (f64::from(x), f64::from(y), f64::from(w), f64::from(h)),
            );
        }
        None => crate::platform::place_mini(window, cx)?,
    }
    Some(saved)
}

/// 退出小屏：先把当前小窗几何落盘（理由见 [`crate::ui_prefs::store_geometry`]），
/// 再按快照还原几何与最大化态。
pub(crate) fn exit(window: &mut Window, cx: &gpui_kit::App, saved: WindowSnapshot) {
    let b = window.bounds();
    crate::ui_prefs::store_geometry(
        crate::ui_prefs::GeometrySlot::Mini,
        b.origin.x.into(),
        b.origin.y.into(),
        b.size.width.into(),
        b.size.height.into(),
    );
    crate::platform::restore(window, cx, saved);
}

/// 钉住窗口（Tauri `set_always_on_top` 同语义：窗口级会话态，大小屏共用
/// 同一个开关，退出小屏不清）。返回成败，调用方**仅在成功时**翻状态——
/// 与 Tauri「先调后端再落 store」同口径。
pub(crate) fn set_topmost(window: &Window, cx: &gpui_kit::App, on: bool) -> bool {
    crate::platform::set_topmost(window, cx, on)
}

// —— 小屏态广播 ——

/// 小屏开着：HomeView 写，Shell 读（侧栏在小屏里藏掉，Tauri use-mini-window
/// 「侧栏隐藏」同款）。窗口缩放本身会触发全树重渲染，Shell 无需额外通知。
static MINI_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_active(on: bool) {
    MINI_ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn is_active() -> bool {
    MINI_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 预览专用快照（screenshot feature）：无头环境没有真实窗口几何可存，
/// 给 1024×680 占位矩形——退出小屏的还原路径在预览里不会被走到。
#[cfg(feature = "screenshot")]
pub(crate) fn preview_geometry() -> WindowSnapshot {
    WindowSnapshot {
        rect: crate::platform::OuterRect::new(0.0, 0.0, 1024.0, 680.0),
        maximized: false,
    }
}
