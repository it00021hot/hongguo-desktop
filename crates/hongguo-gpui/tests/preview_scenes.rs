//! 预览场景矩阵的覆盖率断言：场景名唯一、矩阵把每个场景键都编排了、
//! 基底页面双主题不缺位。纯数据断言，不出图、不碰窗口。
//!
//! scenes.rs 按源文件原样引入——壳是纯 bin（无 lib 目标，集成测试进不了
//! crate 内部；bin 又 `test = false`，模块内 `#[cfg(test)]` 永远不跑），
//! 源文件级引入是唯一通路，scenes.rs 因此保持自洽不引 `super::*`。

#[path = "../src/preview/scenes.rs"]
mod scenes;

use gpui_kit::component::theme::ThemeMode;

/// 场景键全集（种子分发消费的键；矩阵必须把每个键都至少排一次）。
const SCENE_KEYS: [&str; 48] = [
    "feed",
    "picker",
    "picker-series",
    "downloads",
    "comments",
    "login",
    "dm",
    "error",
    "empty",
    "mini",
    "rank",
    "rank-empty",
    "rank-filter",
    "storage",
    "storage-empty",
    "merge",
    "merge-empty",
    "liked",
    "liked-empty",
    "liked-manage",
    "collections",
    "collections-empty",
    "collections-manage",
    "history",
    "history-empty",
    "history-manage",
    "detail",
    "detail-related",
    "detail-upcoming",
    "reservation",
    "reservation-offline",
    "reservation-empty",
    "follows",
    "follows-empty",
    "follows-manage",
    "follows-notices",
    "creator",
    "creator-empty",
    "tasks",
    "tasks-empty",
    "tasks-manage",
    "settings",
    "settings-no-ffmpeg",
    "browse",
    "browse-search",
    "browse-empty",
    "new",
    "new-calendar",
];

#[test]
fn scene_names_are_unique() {
    let mut names: Vec<_> = scenes::cases().into_iter().map(|c| c.name).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "场景名重复: {names:?}");
}

#[test]
fn every_scene_key_is_scheduled() {
    let cases = scenes::cases();
    for key in SCENE_KEYS {
        assert!(
            cases.iter().any(|c| c.scene == key),
            "矩阵缺场景键 {key}（种子分发支持的场景与 cases() 失同步）"
        );
    }
}

#[test]
fn base_pages_have_both_themes() {
    let cases = scenes::cases();
    for key in ["feed", "rank"] {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            assert!(
                cases.iter().any(|c| c.scene == key && c.mode == mode),
                "基底场景 {key} 缺 {mode:?} 主题"
            );
        }
    }
}

#[test]
fn branch_states_are_covered() {
    let cases = scenes::cases();
    for key in ["mini", "empty", "error", "rank-empty"] {
        assert!(cases.iter().any(|c| c.scene == key), "分支态 {key} 缺场景");
    }
}

#[test]
fn mini_scene_uses_player_window_size() {
    let mini = scenes::cases()
        .into_iter()
        .find(|c| c.scene == "mini")
        .expect("缺 mini 场景");
    // 小屏 = 同窗缩到 480×270（crate::mini 的 MINI_W/MINI_H 同参）
    assert_eq!((mini.width, mini.height), (480., 270.));
}

#[test]
fn font_scales_are_covered() {
    let cases = scenes::cases();
    assert!(cases.iter().any(|c| c.font_scale > 1.0), "缺字号缩放场景");
}
