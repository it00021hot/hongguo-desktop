//! 组件库 Button 的壳：一处收口「可点 = 手型」的光标策略（组件库只给
//! link/text 变体手型，其余固定 `cursor_default`）。调用点零改动：import 换成
//! `use crate::ui_button::button as Button;` 即可，变体/链式样式全部照旧。
//! 禁用态回箭头：组件库把光标写死在根样式上，由调用方在 `.disabled(cond)`
//! 之后补 `.when(cond, |b| b.cursor_default())`。

pub(crate) mod button {
    use gpui_kit::ElementId;
    use gpui_kit::component::button::Button;
    use gpui_kit::prelude::*;

    /// 组件库 Button 的构造入口：与组件库同名同义，额外带上「可点 = 手型」。
    /// 调用方样式在组件库链之后落盘，这里设的光标盖得住内部固定的 `cursor_default`。
    pub(crate) fn new(id: impl Into<ElementId>) -> Button {
        Button::new(id).cursor_pointer()
    }
}
