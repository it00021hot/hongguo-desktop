//! 窗口恢复判定（纯函数）回归。壳是 bin，没有 lib；`#[path]` 源文件级引入
//! 与 `preview_scenes` 同一套做法。

#[path = "../src/window_restore.rs"]
mod window_restore;

use window_restore::{
    MIN_RESTORE_H, MIN_RESTORE_W, MINI_RESTORE_H, MINI_RESTORE_W, PORTRAIT_RESTORE_H,
    PORTRAIT_RESTORE_W, restore_rect,
};

const MAIN: (f32, f32, f32, f32) = (0.0, 0.0, 3440.0, 1440.0);

#[test]
fn restores_a_window_smaller_than_the_default() {
    assert_eq!(
        restore_rect(
            200.0,
            200.0,
            900.0,
            600.0,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        Some((200.0, 200.0, 900.0, 600.0)),
        "900×600 是用户正常缩小的尺寸，不能因为小于默认 1280×840 就丢"
    );
    assert_eq!(
        restore_rect(
            1200.0,
            300.0,
            1100.0,
            700.0,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        Some((1200.0, 300.0, 1100.0, 700.0))
    );
}

#[test]
fn accepts_the_interactive_minimum() {
    assert_eq!(
        restore_rect(
            0.0,
            0.0,
            MIN_RESTORE_W,
            MIN_RESTORE_H,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        Some((0.0, 0.0, MIN_RESTORE_W, MIN_RESTORE_H))
    );
    assert_eq!(
        restore_rect(
            0.0,
            0.0,
            MIN_RESTORE_W - 1.0,
            MIN_RESTORE_H,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        None
    );
    assert_eq!(
        restore_rect(
            0.0,
            0.0,
            MIN_RESTORE_W,
            MIN_RESTORE_H - 1.0,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        None
    );
}

#[test]
fn rejects_offscreen_and_non_finite_geometry() {
    assert_eq!(
        restore_rect(
            5000.0,
            300.0,
            1100.0,
            700.0,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        None
    );
    assert_eq!(
        restore_rect(
            -3000.0,
            300.0,
            1100.0,
            700.0,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        None
    );
    assert_eq!(
        restore_rect(
            f32::NAN,
            0.0,
            900.0,
            600.0,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        None
    );
    assert_eq!(
        restore_rect(
            0.0,
            0.0,
            f32::INFINITY,
            600.0,
            &[MAIN],
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        None
    );
    assert_eq!(
        restore_rect(0.0, 0.0, 900.0, 600.0, &[], MIN_RESTORE_W, MIN_RESTORE_H),
        None
    );
}

#[test]
fn any_display_intersection_is_enough() {
    let displays = [(0.0, 0.0, 1280.0, 800.0), (1280.0, 0.0, 1920.0, 1080.0)];
    assert_eq!(
        restore_rect(
            1500.0,
            100.0,
            1000.0,
            700.0,
            &displays,
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        Some((1500.0, 100.0, 1000.0, 700.0)),
        "第二块屏上的窗口不能被第一块屏的判定拒绝"
    );
    assert_eq!(
        restore_rect(
            1200.0,
            200.0,
            900.0,
            600.0,
            &displays,
            MIN_RESTORE_W,
            MIN_RESTORE_H
        ),
        Some((1200.0, 200.0, 900.0, 600.0)),
        "跨两块屏的窗口按相交恢复"
    );
}

#[test]
fn mini_and_portrait_restore_below_the_normal_floor() {
    // 小屏恢复下限是 360×180：常态判定（480×270）会拒掉的最小尺寸小窗，
    // 按小屏下限要能恢复
    assert_eq!(
        restore_rect(
            100.0,
            100.0,
            MINI_RESTORE_W,
            MINI_RESTORE_H,
            &[MAIN],
            MINI_RESTORE_W,
            MINI_RESTORE_H
        ),
        Some((100.0, 100.0, MINI_RESTORE_W, MINI_RESTORE_H))
    );
    assert_eq!(
        restore_rect(
            100.0,
            100.0,
            MINI_RESTORE_W - 1.0,
            MINI_RESTORE_H,
            &[MAIN],
            MINI_RESTORE_W,
            MINI_RESTORE_H
        ),
        None
    );
    // 竖屏恢复下限是 360×400：竖条宽不过常态下限，按竖屏下限要能恢复
    assert_eq!(
        restore_rect(
            100.0,
            100.0,
            PORTRAIT_RESTORE_W,
            PORTRAIT_RESTORE_H,
            &[MAIN],
            PORTRAIT_RESTORE_W,
            PORTRAIT_RESTORE_H
        ),
        Some((100.0, 100.0, PORTRAIT_RESTORE_W, PORTRAIT_RESTORE_H))
    );
    assert_eq!(
        restore_rect(
            100.0,
            100.0,
            PORTRAIT_RESTORE_W,
            PORTRAIT_RESTORE_H - 1.0,
            &[MAIN],
            PORTRAIT_RESTORE_W,
            PORTRAIT_RESTORE_H
        ),
        None
    );
}
