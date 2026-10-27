//! 顶栏与页面筛选共用的胶囊 tab。
//!
//! 尺寸和状态直接对齐 Tauri `TopBarTab`：
//! `rounded-full px-3 py-1 text-xs`，选中为 primary 实底，未选中为 muted
//! 文字 + hover accent。GPUI 通用 Button 的 small 尺寸不是胶囊，调用点各写
//! 一遍会再次漂移，所以只在主题令牌上收口一次。

use gpui_kit::component::button::{Button as ComponentButton, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, FontWeight, SharedString, px};

pub(crate) fn pill_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &App,
) -> ComponentButton {
    let theme = cx.theme();
    // 选中态必须用标准 primary：Custom 变体会把填充色与透明色混合，
    // 白色胶囊会被压成灰色。未选中态用 Custom 只为把 idle 文字压到
    // muted、hover 拉回 accent；填充本身是全透明，不存在混色问题。
    let inactive = ButtonCustomVariant::new(cx)
        .color(theme.transparent)
        .foreground(theme.muted_foreground)
        .hover(theme.accent)
        .active(theme.accent);

    ComponentButton::new(id)
        // 组件库 Button 只给 link/text 变体设手型光标，其余变体固定箭头——
        // 胶囊 tab 是最常点的控件，光标必须是手型（与 Tauri 侧全局
        // `button:not(:disabled){cursor:pointer}` 同款）。
        .cursor_pointer()
        .label(label)
        .small()
        // ButtonRounded 没有 Full 档；胶囊必须大于任一实际高度的一半。
        .rounded(px(999.0))
        .px_3()
        .text_xs()
        .when(active, |button| {
            button.primary().font_weight(FontWeight::MEDIUM)
        })
        .when(!active, |button| button.custom(inactive))
}
