//! 顶栏（自绘）可点控件的命中回归：拖拽 handler 挂在 bar 上、中部 tab 行
//! 是绝对定位的居中覆盖层——两者叠加时按钮的 `on_click` 必须照常到达
//! （组件库 TitleBar 的整条 Drag 区就栽在这，见 shell.rs `render_toolbar`
//! 头注释）。
//!
//! 需要 gpui 的 test-support（`screenshot` feature 已连带开启），运行方式：
//! `cargo test -p hongguo-gpui --features screenshot --test toolbar_click`

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{MouseButton, PlatformInput, Point, Render, px};

struct Host {
    tab_clicks: [Rc<Cell<u32>>; 3],
    pin_clicks: Rc<Cell<u32>>,
    /// 拖拽记账位（真实顶栏同款：按下记起点、抬起清）
    drag: Option<gpui_kit::Point<gpui_kit::Pixels>>,
}

impl Render for Host {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let labels = ["漫剧", "真人剧", "推荐"];
        h_flex()
            .id("test-toolbar")
            .relative()
            .h(px(44.0))
            .w_full()
            .items_center()
            .pl(px(12.0))
            // —— 拖拽 handler（真实顶栏同款）——
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _, _| {
                    this.drag = Some(ev.position);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.drag = None;
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &gpui_kit::MouseMoveEvent, _, _| {
                if let Some(start) = this.drag
                    && (ev.position.x - start.x).as_f32().abs() > 4.0
                {
                    this.drag = None;
                }
            }))
            // —— 中部 tab 行：绝对定位居中覆盖层（真实顶栏同款）——
            .child(
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .h_full()
                    .justify_center()
                    .items_center()
                    .child(h_flex().id("test-tabs").gap_1().children(
                        labels.iter().enumerate().map(|(i, label)| {
                            let clicks = Rc::clone(&self.tab_clicks[i]);
                            Button::new(format!("tab-{label}"))
                                .label(*label)
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    clicks.set(clicks.get() + 1);
                                    cx.stop_propagation();
                                }))
                        }),
                    )),
            )
            // —— 右组（真实顶栏同款：钉在右端）——
            .child(h_flex().ml_auto().h_full().items_center().child(
                Button::new("pin").label("P").small().ghost().on_click({
                    let clicks = Rc::clone(&self.pin_clicks);
                    cx.listener(move |_, _, _, cx| {
                        clicks.set(clicks.get() + 1);
                        cx.stop_propagation();
                    })
                }),
            ))
    }
}

#[gpui_kit::test]
async fn toolbar_controls_are_clickable(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let tab_clicks = [
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
    ];
    let pin_clicks = Rc::new(Cell::new(0));
    let window = cx.add_window({
        let tab_clicks = tab_clicks.clone();
        let pin_clicks = Rc::clone(&pin_clicks);
        move |_, _cx| Host {
            tab_clicks,
            pin_clicks,
            drag: None,
        }
    });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    // 沿顶栏高度带横扫点击（不猜几何）：三只 tab + 右侧 pin 都应命中，
    // 且落空处不得触发任何按钮
    vx.update(|window, cx| {
        for y in [12.0, 21.0, 30.0] {
            let mut x = 4.0;
            while x < 1916.0 {
                let pos = Point::new(px(x), px(y));
                for input in [
                    PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                        position: pos,
                        pressed_button: None,
                        modifiers: Default::default(),
                    }),
                    PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                        button: MouseButton::Left,
                        position: pos,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                        button: MouseButton::Left,
                        position: pos,
                        modifiers: Default::default(),
                        click_count: 1,
                    }),
                ] {
                    window.dispatch_event(input, cx);
                }
                x += 6.0;
            }
        }
    });
    let tab_hits: u32 = tab_clicks.iter().map(|c| c.get()).sum();
    assert!(
        tab_hits >= 3,
        "横扫应命中全部三只 tab（合计 {tab_hits} 次）"
    );
    assert!(pin_clicks.get() >= 1, "右侧按钮应至少命中一次");
}
