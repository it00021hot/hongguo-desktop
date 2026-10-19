//! 上次窗口几何的恢复判定（纯函数，供 `main.rs` 与集成测试共用）。
//!
//! 只做两件事：尺寸不小于交互壳的最小值，且与任一显示器相交。显示器拔掉/
//! 改布局后的旧坐标不能把窗口丢到屏外找不回；但用户正常缩到 900×600 这种
//! 小于默认尺寸的窗也必须能恢复——所以下限用 Tauri 同款的 480×270，而不是
//! 默认窗口的 1024×680。

/// 交互壳的最小可恢复尺寸（与 Tauri `MAIN_MIN` 同款；mini 模式也是这个尺寸）。
pub(crate) const MIN_RESTORE_W: f32 = 480.0;
pub(crate) const MIN_RESTORE_H: f32 = 270.0;

/// 显示器矩形 `(x, y, w, h)`，逻辑像素。
pub(crate) type DisplayRect = (f32, f32, f32, f32);

/// 返回要恢复的窗口矩形；无记忆/过小/屏外返回 `None`（调用方回中置默认）。
pub(crate) fn restore_rect(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    displays: &[DisplayRect],
) -> Option<DisplayRect> {
    if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
        return None;
    }
    if w < MIN_RESTORE_W || h < MIN_RESTORE_H {
        return None;
    }
    let overlaps = displays
        .iter()
        .any(|&(dx, dy, dw, dh)| dx < x + w && x < dx + dw && dy < y + h && y < dy + dh);
    overlaps.then_some((x, y, w, h))
}
