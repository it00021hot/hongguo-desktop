//! 组件库 Button 的壳：一处收口「可点 = 手型」的光标策略。
//!
//! 组件库的 `Button` 只给 link/text 变体设 `cursor_pointer`，其余变体
//! （ghost/primary/outline/custom/danger）固定 `cursor_default`——鼠标停在
//! 可点按钮上看不出能不能点。Tauri 侧一刀切靠全局 CSS（`src/styles/index.css`：
//! `button:not(:disabled) { cursor: pointer; }`），壳没有全局样式表，收口在
//! 这里。
//!
//! 调用点零改动：把组件库 Button 的 import 换成
//! `use crate::ui_button::button as Button;`，`Button::new(...)` 与组件库同名
//! 同义（变体、链式样式、`ButtonVariants` 全部照旧），多的只是那条光标策略。
//!
//! 禁用态：组件库的 disabled 按钮不响应点击，光标要回箭头（与 Tauri 的
//! `:not(:disabled)` 同款）。组件库把光标写死在根样式上，构造期无从得知
//! 禁用与否，所以由调用方在 `.disabled(cond)` 之后补一句
//! `.when(cond, |b| b.cursor_default())`。

pub(crate) mod button {
    use gpui_kit::ElementId;
    use gpui_kit::component::button::Button;
    use gpui_kit::prelude::*;

    /// 组件库 Button 的构造入口：与组件库同名同义，额外带上「可点 = 手型」。
    ///
    /// 调用方的样式在组件库链之后 `refine_style` 落盘（组件库把调用方样式
    /// 克隆成 `instance_style` 再最后应用），所以这里设的光标盖得住组件库
    /// 内部固定的 `cursor_default`。
    pub(crate) fn new(id: impl Into<ElementId>) -> Button {
        Button::new(id).cursor_pointer()
    }
}
