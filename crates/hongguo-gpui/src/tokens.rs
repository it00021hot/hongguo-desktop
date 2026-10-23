//! 业务语义色令牌（对齐 Tauri `src/styles/index.css` 的 `--hot` / `--star` /
//! `--warm`）：热度、评分星、热量数值这类「业务语义色」不在组件库调色板
//! 里，集中放这里，各页只引用不写字面量——Tauri 侧改令牌值，这边一处跟。
//!
//! 每个令牌给出 CSS 的亮/暗两档（都是 Tailwind 调色板同值色，CSS 里写的
//! 就是 oklch 原文），按当前主题档取用——GPUI 壳支持跟随系统/亮/暗三档，
//! 令牌不跟着分档就会在另一种模式下发暗或发灰。

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla};

/// 把 GPUI Kit 的默认主题收口到 Tauri 壳的语义令牌。
///
/// 组件库默认把 `group_box`、`sidebar` 等项目表面画成 `neutral-950`，而
/// Tauri 的 `bg-card` / `bg-sidebar` 是 `neutral-900`；只切 `ThemeMode`
/// 会让壳的底色、侧栏和卡片整体发黑。所有模式切换都必须经过这里，避免
/// 首帧、设置页和跟随系统三条路径各自漂移。
pub(crate) fn apply(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
    Theme::update(cx, |theme| {
        let dark = mode.is_dark();
        let background = color(dark, 0x0a0a0a, 0xffffff);
        let card = color(dark, 0x171717, 0xffffff);
        let foreground = color(dark, 0xfafafa, 0x0a0a0a);
        let muted_surface = color(dark, 0x262626, 0xf5f5f5);
        let muted_foreground = color(dark, 0xa3a3a3, 0x737373);
        let primary = color(dark, 0xfafafa, 0x171717);
        let primary_foreground = color(dark, 0x171717, 0xfafafa);
        let sidebar = color(dark, 0x171717, 0xfafafa);
        let border = if dark {
            gpui_kit::rgba(0xffffff1f).into()
        } else {
            gpui_kit::rgba(0xe5e5e5ff).into()
        };
        let input = if dark {
            gpui_kit::rgba(0xffffff29).into()
        } else {
            gpui_kit::rgba(0xe5e5e5ff).into()
        };
        let danger = color(dark, 0xf87171, 0xef4444);
        let success = color(dark, 0x22c55e, 0x22c55e);
        let warning = color(dark, 0xf59e0b, 0xf59e0b);

        theme.colors.background = background;
        theme.colors.foreground = foreground;
        theme.colors.group_box = card;
        theme.colors.group_box_foreground = foreground;
        theme.colors.popover = card;
        theme.colors.popover_foreground = foreground;
        theme.colors.list = card;
        theme.colors.list_even = background;
        theme.colors.list_head = background;
        theme.colors.table = card;
        theme.colors.table_even = background;
        theme.colors.table_head = background;
        theme.colors.primary = primary;
        theme.colors.primary_foreground = primary_foreground;
        theme.colors.primary_hover = primary;
        theme.colors.primary_active = primary;
        theme.colors.button_primary = primary;
        theme.colors.button_primary_foreground = primary_foreground;
        theme.colors.button_primary_hover = primary;
        theme.colors.button_primary_active = primary;
        theme.colors.secondary = muted_surface;
        theme.colors.secondary_foreground = foreground;
        theme.colors.secondary_hover = muted_surface;
        theme.colors.secondary_active = muted_surface;
        theme.colors.muted = muted_surface;
        theme.colors.muted_foreground = muted_foreground;
        theme.colors.accent = muted_surface;
        theme.colors.accent_foreground = foreground;
        theme.colors.border = border;
        theme.colors.input = input;
        theme.colors.ring = color(dark, 0x737373, 0xa3a3a3);
        theme.colors.sidebar = sidebar;
        theme.colors.sidebar_foreground = foreground;
        theme.colors.sidebar_primary = primary;
        theme.colors.sidebar_primary_foreground = primary_foreground;
        theme.colors.sidebar_accent = muted_surface;
        theme.colors.sidebar_accent_foreground = foreground;
        theme.colors.sidebar_border = border;
        theme.colors.title_bar = sidebar;
        theme.colors.title_bar_border = border;
        theme.colors.status_bar = sidebar;
        theme.colors.status_bar_border = border;
        theme.colors.skeleton = muted_surface;
        theme.colors.switch = color(dark, 0x404040, 0xd4d4d4);
        theme.colors.danger = danger;
        theme.colors.danger_foreground = color(dark, 0xfafafa, 0xfafafa);
        theme.colors.success = success;
        theme.colors.success_foreground = color(dark, 0xfafafa, 0xfafafa);
        theme.colors.warning = warning;
        theme.colors.warning_foreground = color(dark, 0xfafafa, 0xfafafa);

        // Tauri：--radius=10（lg），--radius-md=8（控件），--radius-xl=14（卡片）。
        theme.radius = gpui_kit::px(8.0);
        theme.radius_lg = gpui_kit::px(14.0);
    });
}

fn color(dark: bool, dark_value: u32, light_value: u32) -> Hsla {
    let value = if dark { dark_value } else { light_value };
    // `rgba` 取 0xRRGGBBAA；这里的语义令牌只写 RGB，统一补不透明 alpha。
    gpui_kit::rgba((value << 8) | 0xff).into()
}

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
