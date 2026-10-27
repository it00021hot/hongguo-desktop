//! 竖屏窗口几何的纯逻辑断言（对照 Tauri `fit_portrait` 与
//! `use-portrait-window` 的口径）。
//!
//! geom.rs 按源文件原样引入——壳是纯 bin（无 lib 目标，集成测试进不了
//! crate 内部；bin 又 `test = false`，模块内 `#[cfg(test)]` 永远不跑），
//! 源文件级引入是唯一通路，geom.rs 因此保持自洽不引 `super::*`。

#[path = "../src/portrait/geom.rs"]
mod geom;

use geom::{FitInput, aspect_of_video, fit_rect};

/// 9:16 竖屏内容 → 手机形地板（9/19.5），比内容更长的留边形态。
#[test]
fn aspect_floors_at_phone_shape() {
    let phone = 9.0 / 19.5;
    assert!(
        (aspect_of_video(1080, 1920) - phone).abs() < 1e-9,
        "9:16 钳到手机比例"
    );
    assert!(
        (aspect_of_video(0, 0) - phone).abs() < 1e-9,
        "无尺寸回落手机比例"
    );
    // 横屏/方形不算竖屏比，同样回落手机比例
    assert!((aspect_of_video(1920, 1080) - phone).abs() < 1e-9);
    assert!((aspect_of_video(1080, 1080) - phone).abs() < 1e-9);
    // 比手机更长的内容按自身比例（会被工作区高钳住，但不抬到手机比例）
    let long = 9.0 / 21.0;
    assert!((aspect_of_video(1080, 2520) - long).abs() < 1e-9);
}

/// 普通档：高度取当前高（上限工作区高 × 0.7）、宽 = 高 × 比例、x 居中于
/// **当前窗口**、y 不动。
#[test]
fn normal_fit_keeps_height_and_centers_on_window() {
    let phone = 9.0 / 19.5;
    // 工作区 2560×1440，当前窗口 1280×1392 在 (640, 24)
    let (x, y, w, h) = fit_rect(FitInput {
        aspect: phone,
        full: false,
        cur: (640.0, 24.0, 1280.0, 1392.0),
        work: (0.0, 0.0, 2560.0, 1440.0),
    });
    let expect_h = (1440.0_f64 * 0.7).round();
    assert_eq!(h, expect_h, "普通档高度吃到工作区高的 70%");
    assert_eq!(w, (expect_h * phone).round());
    assert_eq!(y, 24.0, "普通档 y 不动");
    assert_eq!(x, 640.0 + ((1280.0 - w) / 2.0).round(), "x 居中于当前窗口");
}

/// 铺满档：撑满工作区高、居中于工作区、贴工作区顶。
#[test]
fn full_fit_fills_work_area() {
    let phone = 9.0 / 19.5;
    let (x, y, w, h) = fit_rect(FitInput {
        aspect: phone,
        full: true,
        cur: (640.0, 24.0, 1280.0, 600.0),
        work: (0.0, 0.0, 2560.0, 1440.0),
    });
    assert_eq!(h, 1440.0);
    assert_eq!(w, (1440.0 * phone).round());
    assert_eq!(y, 0.0);
    assert_eq!(x, ((2560.0 - w) / 2.0).round());
}

/// 宽溢出工作区时反推高度；且宽度不会小于竖屏最小宽（360）。
#[test]
fn width_clamps_into_work_area_and_min() {
    // 极窄工作区：1024×1440，比例 9/19.5 → 铺满档宽会超
    let phone = 9.0 / 19.5;
    let (_, _, w, h) = fit_rect(FitInput {
        aspect: phone,
        full: true,
        cur: (0.0, 0.0, 400.0, 800.0),
        work: (0.0, 0.0, 500.0, 1440.0),
    });
    assert_eq!(w, 500.0, "宽钳到工作区宽");
    assert_eq!(h, (500.0 / phone).round(), "高度按比例反推");
    // 极小工作区：宽不缩到最小宽以下（宁可溢出 1px 也不做牙签条）
    let (_, _, w, _) = fit_rect(FitInput {
        aspect: phone,
        full: false,
        cur: (0.0, 0.0, 100.0, 100.0),
        work: (0.0, 0.0, 200.0, 300.0),
    });
    assert_eq!(w, 360.0);
}
