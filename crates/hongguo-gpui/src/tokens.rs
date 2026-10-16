//! 业务语义色令牌（对齐 Tauri `src/styles/index.css` 的 `--hot` / `--star` /
//! `--warm`）：热度、评分星、热量数值这类「业务语义色」不在组件库调色板
//! 里，集中放这里，各页只引用不写字面量——Tauri 侧改令牌值，这边一处跟。
//!
//! 取的是 CSS 里的**暗色档**：GPUI 壳当前固定深色（`main.rs` 的
//! `Theme::change(ThemeMode::Dark)`，跟随系统是后话）。各值都是 Tailwind
//! 调色板的同值色（CSS 里写的就是 oklch 原文），备注标了对应档位。
//!
//! 现状只有 `warm` 收编完（找剧页热度/联想命中、榜单推荐语）；`hot` /
//! `star` 仍散在各页裸色值（0xfacc15 / 0xef4444 一族），待一并收编。

/// warm：热度数值/计数、联想命中片段（暗色档 = orange-400）。
/// 亮色档是 orange-500 `oklch(0.705 0.213 47.604)`。
pub(crate) fn warm() -> gpui_kit::Rgba {
    gpui_kit::rgba(0xfb923cff)
}
