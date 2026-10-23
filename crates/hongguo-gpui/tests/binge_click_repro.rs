//! 「观看完整短剧」横条被底部控制条透明热区盖住的复现。
//!
//! 真实舞台层序：画面 → 连播横条（bottom: 56） → 控制条（bottom: 0）。
//! 控制条为了悬停手感带 `pt-10`，这个透明上沿与横条重叠，并在 root 上拦截
//! mouse down/up。测试按同一几何和层序点击横条中心，要求命中横条且不漏到画面。
//!
//! 运行：`cargo test -p hongguo-gpui --features screenshot --test binge_click_repro`

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{MouseButton, PlatformInput, Point, Render, SharedString, div, px};

struct Host {
    binge_clicks: Rc<Cell<u32>>,
    stage_clicks: Rc<Cell<u32>>,
}

impl Render for Host {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let binge_clicks = Rc::clone(&self.binge_clicks);
        let stage_clicks = Rc::clone(&self.stage_clicks);
        v_flex()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(gpui_kit::rgb(0x000000))
            .child(
                div()
                    .id("stage-video")
                    .absolute()
                    .size_full()
                    .on_click(cx.listener(move |_, _, _, _| {
                        stage_clicks.set(stage_clicks.get() + 1);
                    })),
            )
            // 真实控制条：pt-10 的透明上沿覆盖横条，并在 root 拦截 down/up。
            .child(
                v_flex()
                    .id("stage-controls")
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .px_4()
                    .pt_10()
                    .pb_3()
                    .gap_2()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .child(div().w_full().h(px(2.0)).bg(gpui_kit::rgb(0xffffff)))
                    .child(div().w_full().h(px(32.0))),
            )
            // 连播横条必须后画：控制条的 40px 透明悬停缓冲与其重叠，
            // 必须让横条成为更高的命中层。
            .child(
                h_flex()
                    .id("binge-entry")
                    .absolute()
                    .left_3()
                    .right_3()
                    .bottom(px(56.0))
                    .h(px(40.0))
                    .justify_center()
                    .items_center()
                    .cursor_pointer()
                    .child(SharedString::from("观看完整短剧 · 全732集"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        binge_clicks.set(binge_clicks.get() + 1);
                        cx.stop_propagation();
                    })),
            )
    }
}

#[gpui_kit::test]
async fn binge_entry_click_reaches_on_click(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let binge_clicks = Rc::new(Cell::new(0));
    let stage_clicks = Rc::new(Cell::new(0));
    let window = cx.add_window(|_, _cx| Host {
        binge_clicks: Rc::clone(&binge_clicks),
        stage_clicks: Rc::clone(&stage_clicks),
    });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    vx.update(|window, cx| {
        let size = window.viewport_size();
        let position = Point::new(size.width / 2.0, size.height - px(76.0));
        for input in [
            PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }),
            PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }),
            PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count: 1,
            }),
        ] {
            window.dispatch_event(input, cx);
        }
    });
    assert_eq!(binge_clicks.get(), 1, "点击横条中心必须进入连播态");
    assert_eq!(stage_clicks.get(), 0, "横条点击不能漏到画面层");
}
