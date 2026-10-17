//! 竖屏播放的窗口几何（Tauri `window_cmd` 的 portrait 段对齐）：同一个窗口
//! 收窄成竖条、退出按快照还原——与小屏同一套快照机制。
//!
//! 与 Tauri 的逐项对应：进入先退全屏（全屏窗口尺寸归系统管，收窄会被吞）、
//! 最小宽 360、普通档高度最多吃工作区高的 70%、最大化进竖屏直接进铺满档、
//! 铺满档退出还原普通档几何快照、切集/换清晰度导致比例变化 >1% 时重适配。
//!
//! 几何计算（[`geom::fit_rect`]）是两端共用的纯函数；坐标空间与落窗方式
//! 的差异全在 [`crate::platform`]（Windows：物理像素左上原点；macOS：
//! AppKit 点左下原点，由平台实现负责翻转）。本模块零平台分支。

pub(crate) mod geom;

use gpui_kit::Window;

use crate::platform::OuterRect;
use crate::platform::WindowSnapshot;

/// 铺满档退出时要还原的普通档几何（Tauri `PORTRAIT_FULL_RESTORE`
/// 同语义）。坐标空间随平台，业务不解读。
static FULL_RESTORE: std::sync::Mutex<Option<OuterRect>> = std::sync::Mutex::new(None);

/// 竖屏开着：HomeView 写，Shell 读（侧栏在竖屏里藏掉、顶栏保留——Tauri
/// app-shell 同款：`!miniScreen && !playerFullscreen && !portrait` 才渲染侧栏）。
static PORTRAIT_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_active(on: bool) {
    PORTRAIT_ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn is_active() -> bool {
    PORTRAIT_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 进入竖屏：快照 → 按比例收窄落位。几何不可得返回 `None`（调用方保持
/// 原状）。调用方负责幂等与「先退出其它紧凑形态」。
pub(crate) fn enter(
    window: &mut Window,
    aspect: f64,
    cx: &gpui_kit::App,
) -> Option<WindowSnapshot> {
    let saved = crate::platform::snapshot(window)?;
    // 最大化进竖屏：退最大化直接进铺满档（Tauri enter 的 Ev + fit(full)）
    let full = saved.maximized;
    *FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    fit(window, aspect, full, cx);
    Some(saved)
}

/// 按比例重算并落窗（换集/换清晰度的重适配与首次进入共用）。
pub(crate) fn fit(window: &mut Window, aspect: f64, full: bool, cx: &gpui_kit::App) {
    let Some(rect) = crate::platform::portrait_geometry(window, aspect, full) else {
        return;
    };
    crate::platform::apply_frame(window, cx, rect);
}

/// 铺满档切换（Tauri `set_portrait_full` 同款）：普通→铺满先快照当前几何；
/// 铺满→普通还原快照（没有快照——最大化直接进来的——按普通档重算）。
pub(crate) fn set_full(window: &mut Window, aspect: f64, full: bool, cx: &gpui_kit::App) {
    if full {
        if let Some(rect) = crate::platform::current_rect(window) {
            *FULL_RESTORE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(rect);
        }
        fit(window, aspect, true, cx);
        return;
    }
    let saved = FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    match saved {
        Some(rect) => crate::platform::restore_frame(window, cx, rect),
        None => fit(window, aspect, false, cx),
    }
}

/// 退出竖屏：还原进入前的几何与最大化态（丢掉的铺满快照不再需要）。
pub(crate) fn exit(window: &mut Window, saved: WindowSnapshot, cx: &gpui_kit::App) {
    *FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    crate::mini::exit(window, cx, saved);
}
