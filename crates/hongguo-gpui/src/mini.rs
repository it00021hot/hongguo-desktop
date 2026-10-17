//! 小屏播放的窗口几何（Tauri `enter_mini_screen`/`exit_mini_screen` 对齐）：
//! **同一个窗口**缩成 480×270、落到工作区右下角，退出按快照原样还原——
//! 不另开窗口、不藏窗、不暂停，播放零中断（「独立置顶小窗」是 Tauri 侧
//! 已否决的杜撰设计，见 hongguo-tauri `app_cmd.rs` 的注释）。
//!
//! 平台差异全部在 [`crate::platform`]：落位公式、坐标空间、边框换算各自
//! 实现在 windows/macos 子树里，本模块只做业务编排（快照 → 落位 → 还原）。

use gpui_kit::Window;

pub(crate) use crate::platform::WindowSnapshot;

/// 进入小屏：快照 → 缩放落位（工作区右下角，小工作区压到放得下为止）。
/// 几何不可得时返回 `None`，调用方保持原状（与 Tauri 幂等注释同口径：
/// 已在小屏时调用方不得再进来，否则快照会被 480×270 覆盖）。
pub(crate) fn enter(window: &mut Window, cx: &gpui_kit::App) -> Option<WindowSnapshot> {
    let saved = crate::platform::snapshot(window)?;
    crate::platform::place_mini(window, cx)?;
    Some(saved)
}

/// 退出小屏：按快照还原几何与最大化态。
pub(crate) fn exit(window: &mut Window, cx: &gpui_kit::App, saved: WindowSnapshot) {
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
