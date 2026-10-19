//! 窗口恢复判定（纯函数）回归。壳是 bin，没有 lib；`#[path]` 源文件级引入
//! 与 `preview_scenes` 同一套做法。

#[path = "../src/window_restore.rs"]
mod window_restore;

use window_restore::{MIN_RESTORE_H, MIN_RESTORE_W, restore_rect};

const MAIN: (f32, f32, f32, f32) = (0.0, 0.0, 3440.0, 1440.0);

#[test]
fn restores_a_window_smaller_than_the_default() {
    assert_eq!(
        restore_rect(200.0, 200.0, 900.0, 600.0, &[MAIN]),
        Some((200.0, 200.0, 900.0, 600.0)),
        "900×600 是用户正常缩小的尺寸，不能因为小于默认 1280×840 就丢"
    );
    assert_eq!(
        restore_rect(1200.0, 300.0, 1100.0, 700.0, &[MAIN]),
        Some((1200.0, 300.0, 1100.0, 700.0))
    );
}

#[test]
fn accepts_the_interactive_minimum() {
    assert_eq!(
        restore_rect(0.0, 0.0, MIN_RESTORE_W, MIN_RESTORE_H, &[MAIN]),
        Some((0.0, 0.0, MIN_RESTORE_W, MIN_RESTORE_H))
    );
    assert_eq!(
        restore_rect(0.0, 0.0, MIN_RESTORE_W - 1.0, MIN_RESTORE_H, &[MAIN]),
        None
    );
    assert_eq!(
        restore_rect(0.0, 0.0, MIN_RESTORE_W, MIN_RESTORE_H - 1.0, &[MAIN]),
        None
    );
}

#[test]
fn rejects_offscreen_and_non_finite_geometry() {
    assert_eq!(restore_rect(5000.0, 300.0, 1100.0, 700.0, &[MAIN]), None);
    assert_eq!(restore_rect(-3000.0, 300.0, 1100.0, 700.0, &[MAIN]), None);
    assert_eq!(restore_rect(f32::NAN, 0.0, 900.0, 600.0, &[MAIN]), None);
    assert_eq!(restore_rect(0.0, 0.0, f32::INFINITY, 600.0, &[MAIN]), None);
    assert_eq!(restore_rect(0.0, 0.0, 900.0, 600.0, &[]), None);
}

#[test]
fn any_display_intersection_is_enough() {
    let displays = [(0.0, 0.0, 1280.0, 800.0), (1280.0, 0.0, 1920.0, 1080.0)];
    assert_eq!(
        restore_rect(1500.0, 100.0, 1000.0, 700.0, &displays),
        Some((1500.0, 100.0, 1000.0, 700.0)),
        "第二块屏上的窗口不能被第一块屏的判定拒绝"
    );
    assert_eq!(
        restore_rect(1200.0, 200.0, 900.0, 600.0, &displays),
        Some((1200.0, 200.0, 900.0, 600.0)),
        "跨两块屏的窗口按相交恢复"
    );
}
