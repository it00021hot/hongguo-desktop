//! 上次窗口几何的恢复判定（纯函数，供 `main.rs` 与集成测试共用）。
//!
//! 三种窗态（常态/小屏/竖屏）各记各的槽位，恢复判定同源：尺寸不小于该
//! 形态的最小值，且与任一显示器相交。显示器拔掉/改布局后的旧坐标不能把
//! 窗口丢到屏外找不回；但用户正常缩到 900×600 这种小于默认尺寸的窗也
//! 必须能恢复——所以常态下限用 Tauri 同款的 480×270，而不是默认窗口的
//! 1024×680。

/// 常态的最小可恢复尺寸（与 Tauri `MAIN_MIN` 同款；mini 模式也是这个尺寸）。
pub(crate) const MIN_RESTORE_W: f32 = 480.0;
pub(crate) const MIN_RESTORE_H: f32 = 270.0;

/// 小屏的最小可恢复尺寸（Tauri window_cmd 的 `MINI_MIN` 同款：控制条
/// 不换行的下限，可继续缩，恢复不到它以下才回默认落角）。
pub(crate) const MINI_RESTORE_W: f32 = 360.0;
pub(crate) const MINI_RESTORE_H: f32 = 180.0;

/// 竖屏的最小可恢复尺寸（Tauri window_cmd 的 `PORTRAIT_MIN` 同款）。
pub(crate) const PORTRAIT_RESTORE_W: f32 = 360.0;
pub(crate) const PORTRAIT_RESTORE_H: f32 = 400.0;

/// 显示器矩形 `(x, y, w, h)`，逻辑像素（收集入口在 [`crate::platform`]，
/// 本模块保持纯函数）。
pub(crate) type DisplayRect = (f32, f32, f32, f32);

/// 返回要恢复的窗口矩形；无记忆/过小/屏外返回 `None`（调用方回该形态的
/// 默认落位）。`min_w`/`min_h` 按形态传上面的对应常量。
pub(crate) fn restore_rect(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    displays: &[DisplayRect],
    min_w: f32,
    min_h: f32,
) -> Option<DisplayRect> {
    if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
        return None;
    }
    if w < min_w || h < min_h {
        return None;
    }
    let overlaps = displays
        .iter()
        .any(|&(dx, dy, dw, dh)| dx < x + w && x < dx + dw && dy < y + h && y < dy + dh);
    overlaps.then_some((x, y, w, h))
}
