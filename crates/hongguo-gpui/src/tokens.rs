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
        // 表格行分隔线跟壳的边框令牌同色：组件库默认是调色板里的
        // neutral-800/70%，暗色档比壳的 white/12% 深一档，行会显得更重。
        theme.colors.table_row_border = border;
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
        // radius_lg 跟卡片圆角同一档（都是 Tauri 的 rounded-xl），取值走
        // radius 档位，别在这里另写一个 14。
        theme.radius = gpui_kit::px(8.0);
        theme.radius_lg = radius::CARD;
    });
}

/// 圆角档位（物理像素）。
///
/// 与 Tauri `src/styles/index.css` 的 `--radius` 尺度一一对应：根令牌
/// `--radius: 0.625rem`（10px），`@theme inline` 派生出 `--radius-md`
/// （-2px = 8px）、`--radius-lg`（= 10px）、`--radius-xl`（+4px = 14px）。
/// Tailwind 默认那套 6/8/12 不是本项目的值，别按默认值对齐。
///
/// 封面与卡片按用途取档，各页只引用不写像素。
pub(crate) mod radius {
    /// 卡片圆角（14px）——Tauri `rounded-xl`。
    pub(crate) const CARD: gpui_kit::Pixels = gpui_kit::px(14.0);
    /// 行内封面圆角（10px）——Tauri `rounded-lg`。
    pub(crate) const COVER: gpui_kit::Pixels = gpui_kit::px(10.0);
    /// 小图圆角（8px）——Tauri `rounded-md`。
    pub(crate) const THUMB: gpui_kit::Pixels = gpui_kit::px(8.0);
}

/// 封面圆角档：与 Tauri 的 `rounded-*` 一一对应（半径见 [`radius`]）。
///
/// **圆角必须落在图片元素上**：gpui 的 `ContentMask` 是矩形，父容器的圆角
/// 只圆自己的底色、不会裁子件；真正能把图角裁圆的是图片自己的
/// `corner_radii`（`paint_image` 带着它上屏）。容器的圆角只管底色，两者
/// 取同一档才不会露方角。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoverShape {
    /// 网格卡封面：只圆顶部两角——下方接着卡片文字区，底角是方的（Tauri
    /// `rounded-xl` 卡片裁出来的同款效果）。
    Card,
    /// 详情页主封面：四角圆（`rounded-xl`）。
    Detail,
    /// 行内封面：四角圆（`rounded-lg`）。
    Inline,
    /// 小图（选集格、搜索建议缩略图）：四角圆（`rounded-md`）。
    Thumb,
    /// 头像：整圆（`rounded-full`）。
    Circle,
}

impl CoverShape {
    /// 圆角半径（像素）。`Circle` 走整圆（`rounded_full`），没有固定半径，
    /// 这里返回 0 表示「不适用」，别拿它当圆角用。
    pub(crate) fn radius(self) -> gpui_kit::Pixels {
        match self {
            CoverShape::Card | CoverShape::Detail => radius::CARD,
            CoverShape::Inline => radius::COVER,
            CoverShape::Thumb => radius::THUMB,
            CoverShape::Circle => gpui_kit::px(0.0),
        }
    }

    /// 把该档圆角落到元素上。
    pub(crate) fn apply<E: gpui_kit::Styled>(self, element: E) -> E {
        match self {
            CoverShape::Card => element.rounded_t(self.radius()),
            CoverShape::Detail => element.rounded(self.radius()),
            CoverShape::Inline => element.rounded(self.radius()),
            CoverShape::Thumb => element.rounded(self.radius()),
            CoverShape::Circle => element.rounded_full(),
        }
    }
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

/// 封面遮罩底：半透明黑，档位用百分比，直接对应 Tauri 的 `bg-black/NN`
/// （`scrim(60)` ↔ `bg-black/60`），改一处两壳同。
///
/// 贴在封面照片上的底色不跟主题走：亮色档的主题前景是深灰，压在浅色封面上
/// 会糊成一片，所以这一族固定黑白，只有透明度是变量。
///
/// `scrim(0)` = 全透明（渐变末端、播放区打洞）。
pub(crate) fn scrim(percent: u8) -> gpui_kit::Rgba {
    gpui_kit::rgba(alpha(percent))
}

/// 封面上的白字：对应 Tauri 的 `text-white/NN`（`on_media(70)` ↔ `text-white/70`）。
pub(crate) fn on_media(percent: u8) -> gpui_kit::Rgba {
    gpui_kit::rgba(0x00ffffff | alpha(percent))
}

/// 百分比 → alpha 通道（`rgba` 取 0xRRGGBBAA，alpha 在最高字节）。
/// 四舍五入对齐 Tailwind 的 `rgb(0 0 0 / 0.6)` → 153/255。
fn alpha(percent: u8) -> u32 {
    ((percent as u32 * 255 + 50) / 100) << 24
}

/// 键盘焦点的可见指示：Tauri/shadcn 侧是 `focus-visible:border-ring` +
/// `focus-visible:ring-[3px] focus-visible:ring-ring/50`（3px 外圈、ring 色半透明）。
/// gpui 自己没有默认焦点环，gpui-kit 的 `focus_ring_style` 要 `&Window` 才能算圆角，
/// 页面级渲染链拿不到，所以按同款规格用外圈阴影补上。
///
/// 焦点环是全壳主题派生值（不是某个页面的私有件），故归令牌层：任何加进
/// 焦点遍历的控件都从这里取，避免各处各写一遍漂移。
pub(crate) fn focus_ring(theme: &Theme) -> Vec<gpui_kit::BoxShadow> {
    vec![gpui_kit::base::box_shadow(
        gpui_kit::px(0.),
        gpui_kit::px(0.),
        gpui_kit::px(0.),
        gpui_kit::px(3.),
        theme.ring.alpha(0.5),
    )]
}
