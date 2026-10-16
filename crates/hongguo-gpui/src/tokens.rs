//! 业务语义色令牌（对齐 Tauri `src/styles/index.css` 的 `--hot` / `--star` /
//! `--warm`）：热度、评分星、热量数值这类「业务语义色」不在组件库调色板
//! 里，集中放这里，各页只引用不写字面量——Tauri 侧改令牌值，这边一处跟。
//!
//! 每个令牌给出 CSS 的亮/暗两档（都是 Tailwind 调色板同值色，CSS 里写的
//! 就是 oklch 原文），按当前主题档取用——GPUI 壳支持跟随系统/亮/暗三档，
//! 令牌不跟着分档就会在另一种模式下发暗或发灰。

use gpui_kit::component::ThemeMode;

/// hot：热度/爆标/点赞激活。暗色档 = red-400 `oklch(0.704 0.191 22.216)`；
/// 亮色档 = red-500 `oklch(0.637 0.237 25.331)`。
pub(crate) fn hot(mode: ThemeMode) -> gpui_kit::Rgba {
    if mode.is_dark() {
        gpui_kit::rgba(0xf87171ff)
    } else {
        gpui_kit::rgba(0xef4444ff)
    }
}

/// star：评分星/榜单前三。暗色档 = amber-400 `oklch(0.828 0.189 84.429)`；
/// 亮色档 = amber-500 `oklch(0.769 0.188 70.08)`。
pub(crate) fn star(mode: ThemeMode) -> gpui_kit::Rgba {
    if mode.is_dark() {
        gpui_kit::rgba(0xfbbf24ff)
    } else {
        gpui_kit::rgba(0xf59e0bff)
    }
}

/// warm：热度数值/计数、联想命中片段。
/// 暗色档 = orange-400 `oklch(0.75 0.183 55.934)`；亮色档 = orange-500
/// `oklch(0.705 0.213 47.604)`。
pub(crate) fn warm(mode: ThemeMode) -> gpui_kit::Rgba {
    if mode.is_dark() {
        gpui_kit::rgba(0xfb923cff)
    } else {
        gpui_kit::rgba(0xf97316ff)
    }
}
