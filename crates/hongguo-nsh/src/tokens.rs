//! 语义令牌：把 nsh 的默认主题收口到 Tauri 壳的设计令牌
//!（`src/styles/index.css` 的 `--background` / `--card` / `--muted` 等）。
//!
//! 为什么不直接用默认主题：nsh 默认主题是 gpui-kit 风格的中性色，而本应用的
//! 底色/卡片/侧栏在 Tauri 壳里是另一组值（`neutral-950/900` 一档）。不在这里
//! 对齐，三壳的观感会各自漂移。
//!
//! 每个令牌给亮/暗两档（`AdaptiveColor` 由平台按系统外观切换，进程内不做
//! 判断）——与 GPUI 壳 `tokens.rs` 同一份取值。

use nsh_core::ui::prelude::*;

/// 主题（在 `Root::theme` 里交给 nsh，首帧即是应用自己的配色）。
pub fn theme() -> Theme {
    let mut theme = Theme::default();
    let colors = &mut theme.colors;
    colors.background = adaptive(0xffffff, 0x0a0a0a);
    colors.foreground = adaptive(0x0a0a0a, 0xfafafa);
    colors.group_box = adaptive(0xffffff, 0x171717);
    colors.group_box_foreground = adaptive(0x0a0a0a, 0xfafafa);
    colors.muted = adaptive(0xf5f5f5, 0x262626);
    colors.muted_foreground = adaptive(0x737373, 0xa3a3a3);
    colors.primary = adaptive(0x171717, 0xfafafa);
    colors.primary_foreground = adaptive(0xfafafa, 0x171717);
    colors.border = adaptive(0xe5e5e5, 0x2a2a2a);
    colors.accent = adaptive(0xf5f5f5, 0x262626);
    colors.accent_foreground = adaptive(0x171717, 0xfafafa);
    colors.list = adaptive(0xffffff, 0x0f0f0f);
    colors.list_head = adaptive(0xfafafa, 0x141414);
    colors.danger = adaptive(0xef4444, 0xf87171);
    colors.success = adaptive(0x22c55e, 0x22c55e);
    colors.warning = adaptive(0xf59e0b, 0xf59e0b);
    theme.radius = 8.0;
    theme.radius_lg = 12.0;
    theme.font_size = 14.0;
    theme
}

/// 模态遮罩（恒黑透明，亮暗主题同值；GPUI 壳的登录/下载遮罩同款）。
pub fn scrim() -> AdaptiveColor {
    let color = rgba(0x00000099);
    AdaptiveColor::new(color, color)
}

/// 画布图标的弱化色（canvas 只收 `Color`，吃不到 AdaptiveColor 的双档；
/// 取暗色档的 muted 值，亮色主题下作中性灰也不突兀）。
pub fn icon_muted() -> Color {
    rgba(0xa3a3a3ff)
}

/// 业务语义色（对齐 CSS 的 `--hot` / `--star` / `--warm`）。页面迁进来之前
/// 没有调用方，先按 P1 各页的既定用法留在令牌层（业务色就该只有这一处定义）。
#[expect(dead_code, reason = "业务色随 P1 页面迁入启用，定义先立在令牌层")]
pub mod business {
    use super::*;

    /// 热度。
    pub fn hot() -> AdaptiveColor {
        adaptive(0xef4444, 0xf87171)
    }

    /// 评分星。
    pub fn star() -> AdaptiveColor {
        adaptive(0xeab308, 0xfacc15)
    }

    /// 更新/预约等暖色强调。
    pub fn warm() -> AdaptiveColor {
        adaptive(0xf97316, 0xfb923c)
    }
}

fn adaptive(light: u32, dark: u32) -> AdaptiveColor {
    AdaptiveColor::new(rgb(light), rgb(dark))
}
