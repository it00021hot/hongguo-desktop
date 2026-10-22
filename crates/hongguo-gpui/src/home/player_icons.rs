//! 播放器控制栏自绘图标（B 站播放器形态；Tauri `player-icons.tsx` 的 1:1 移植）。
//!
//! 为什么自绘：gpui-kit 的 Lucide 线框与 B 站播放器不是一种语言——弹幕是
//! 「弹」字、网页全屏是方框+内侧四角、小窗是画中画。与 Tauri 互动栏的
//! 抖音系自绘图标同一做法：几何内嵌、颜色由调用方传入（`currentColor` 语义），
//! 不经 AssetSource。
//!
//! 两类实现：
//! - 几何图标（网页全屏/全屏/小窗/关态斜杠）内嵌 SVG 字节交给 `Icon::data`，
//!   与 Tauri 侧共用同一份路径（`assets/player/*.svg`）；
//! - 弹幕两枚是汉字：汉字必须走文本渲染（SVG 里的 `<text>` 依赖系统字体回退，
//!   字形不可控），所以用 div 拼字 + 齿轮角标。
//!
//! 改动时两边一起改：`src/features/player/components/controls/player-icons.tsx`。

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{FontWeight, Hsla, div, px};

/// 网页全屏（无边框）：方框 + 内侧四角。
const WEB_FULLSCREEN: &[u8] = include_bytes!("../../assets/player/web-fullscreen.svg");
/// 全屏：四角折线向外。
const FULLSCREEN: &[u8] = include_bytes!("../../assets/player/fullscreen.svg");
/// 退出全屏：四角折线收拢。
const FULLSCREEN_EXIT: &[u8] = include_bytes!("../../assets/player/fullscreen-exit.svg");
/// 小窗：屏幕 + 右下角小屏 + 指入箭头。
const MINI_WINDOW: &[u8] = include_bytes!("../../assets/player/mini-window.svg");
/// 弹幕开关的关态斜杠（「\」方向）。
const DANMAKU_OFF: &[u8] = include_bytes!("../../assets/player/danmaku-off.svg");

/// 弹幕图标格（px）：与 Tauri 侧 16px 同格，字形 11px 同源。
/// 图标内部几何是栅格化尺寸（不随 rem 缩放），与 gpui-kit `Icon` 的固定档位同义。
const DM_BOX: f32 = 16.0;

/// 网页全屏（无边框）图标。
pub(super) fn web_fullscreen() -> Icon {
    Icon::default().data(WEB_FULLSCREEN)
}

/// 全屏 / 退出全屏图标（`exit` 决定折线朝内还是朝外）。
pub(super) fn fullscreen(exit: bool) -> Icon {
    Icon::default().data(if exit { FULLSCREEN_EXIT } else { FULLSCREEN })
}

/// 小窗（画中画）图标。
pub(super) fn mini_window() -> Icon {
    Icon::default().data(MINI_WINDOW)
}

/// 弹幕开关：B 站同款「弹」字；关闭时加一道反斜杠（颜色由调用方压暗）。
pub(super) fn danmaku_switch(active: bool, color: impl Into<Hsla>) -> impl IntoElement {
    let color = color.into();
    let slash = Icon::default()
        .data(DANMAKU_OFF)
        .absolute()
        .top(px(-1.0))
        .left(px(-1.0))
        .size(px(DM_BOX + 2.0))
        .text_color(color);
    div()
        .relative()
        .size(px(DM_BOX))
        .child(danmaku_glyph(color))
        .when(!active, |this| this.child(slash))
}

/// 弹幕设置：无框「弹」字 + 右下角齿轮角标——与开关同字形，靠角标区分
/// （两个相邻按钮都只写字、再加斜杠就是「关」，方框在 16px 下只是噪点）。
///
/// 角标垫一层深色圆底：否则会压在「弹」的右竖笔上，16px 下糊成一团
/// （Tauri 侧同款处理）。
pub(super) fn danmaku_settings(color: impl Into<Hsla>) -> impl IntoElement {
    let color = color.into();
    let badge = div()
        .absolute()
        .right(px(-4.0))
        .bottom(px(-4.0))
        .size(px(9.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.5))
        .bg(gpui_kit::rgba(0x000000b3))
        .child(
            Icon::new(IconName::Settings)
                .size(px(7.0))
                .text_color(color),
        );
    div()
        .relative()
        .size(px(DM_BOX))
        .child(danmaku_glyph(color))
        .child(badge)
}

/// 弹幕书写入口（手机端同款）：无框「弹」字 + 右下角笔角标——窄舞台下发送
/// 框折叠成的这一钮，点开浮层输入。角标处理与 [`danmaku_settings`] 同款。
pub(super) fn danmaku_write(color: impl Into<Hsla>) -> impl IntoElement {
    let color = color.into();
    let badge = div()
        .absolute()
        .right(px(-4.0))
        .bottom(px(-4.0))
        .size(px(9.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.5))
        .bg(gpui_kit::rgba(0x000000b3))
        .child(Icon::new(IconName::Pen).size(px(7.0)).text_color(color));
    div()
        .relative()
        .size(px(DM_BOX))
        .child(danmaku_glyph(color))
        .child(badge)
}

/// 「弹」字本体（开关与设置共用；无边框，靠斜杠/齿轮角标区分）。
fn danmaku_glyph(color: Hsla) -> impl IntoElement {
    div()
        .size(px(DM_BOX))
        .flex()
        .items_center()
        .justify_center()
        .text_color(color)
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child("弹")
}
