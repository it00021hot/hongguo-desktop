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

/// 竖屏正处在铺满档：几何是**派生态**（按工作区高算出来的），不是用户
/// 拖出来的竖条几何。记忆槽位只收后者——铺满时既不采样也不落槽，否则
/// 退出竖屏时会把铺满几何记成"上次竖屏大小"，下次进竖屏直接满高、
/// 铺满钮看起来永远没反应。
static PORTRAIT_FULL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_full_active(on: bool) {
    PORTRAIT_FULL.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn is_full() -> bool {
    PORTRAIT_FULL.load(std::sync::atomic::Ordering::Relaxed)
}

/// 进入竖屏：快照 → 落位。上次竖屏几何有效（不小于竖屏恢复下限且落在
/// 任一显示器内）就原样回去——三种窗态各记各的槽位，切形态互不覆盖；
/// 素材比例与上次不同由换集重适配（portrait_panel 的 refit 路径）纠偏。
/// 最大化进竖屏仍直进铺满档（Tauri enter 的 Ev + fit(full) 同款）；
/// 无记忆按比例现算。几何不可得返回 `None`（调用方保持原状）。
/// 调用方负责幂等与「先退出其它紧凑形态」。
pub(crate) fn enter(
    window: &mut Window,
    aspect: f64,
    cx: &gpui_kit::App,
) -> Option<WindowSnapshot> {
    let saved = crate::platform::snapshot(window)?;
    // 最大化进竖屏：退最大化直接进铺满档（Tauri enter 的 Ev + fit(full)）
    let full = saved.maximized;
    set_full_active(full);
    *FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    let prefs = crate::ui_prefs::load();
    let remembered = if full {
        None
    } else {
        crate::window_restore::restore_rect(
            prefs.portrait_x,
            prefs.portrait_y,
            prefs.portrait_w,
            prefs.portrait_h,
            &crate::platform::memory_display_rects(cx),
            crate::window_restore::PORTRAIT_RESTORE_W,
            crate::window_restore::PORTRAIT_RESTORE_H,
        )
    };
    match remembered {
        Some((x, y, w, h)) => {
            crate::platform::memory_restore_frame(
                window,
                cx,
                crate::platform::OuterRect::new(
                    f64::from(x),
                    f64::from(y),
                    f64::from(w),
                    f64::from(h),
                ),
            );
        }
        None => fit(window, aspect, full, cx),
    }
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
    set_full_active(full);
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

/// 退出竖屏：先把当前竖屏几何落盘（理由同 [`crate::mini::exit`]，写竖屏
/// 自己的槽位），再还原进入前的几何与最大化态（丢掉的铺满快照不再需要）。
///
/// **铺满中退出不落盘**：那一份是派生的满高几何，记进槽位会让下次进竖屏
/// 直接满高、铺满钮看起来永远没反应（实测：在铺满里退竖屏 → 再进竖屏
/// 得到 651×1410 而不是 380×700）。
pub(crate) fn exit(window: &mut Window, saved: WindowSnapshot, cx: &gpui_kit::App) {
    let was_full = is_full();
    set_full_active(false);
    *FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    if !was_full && let Some(rect) = crate::platform::memory_rect(window) {
        crate::ui_prefs::store_geometry(
            crate::ui_prefs::GeometrySlot::Portrait,
            rect.x as f32,
            rect.y as f32,
            rect.w as f32,
            rect.h as f32,
        );
    }
    crate::platform::restore(window, cx, saved);
}
