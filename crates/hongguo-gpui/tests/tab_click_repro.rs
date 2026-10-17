//! 舞台分类 tab「点不动」的复现测试：按真实舞台的层序与结构搭最小模型
//! （画面层 → 悬浮条状层 → tab 条 → 控制栏），点真实的组件 Button，
//! 裁决 tab 的 on_click 是否到达、是否漏到画面层。
//!
//! 需要 gpui 的 test-support（`screenshot` feature 已连带开启），运行方式：
//! `cargo test -p hongguo-gpui --features screenshot --test tab_click_repro`

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{MouseButton, PlatformInput, Point, Render, div, px};

struct Host {
    tab_clicks: [Rc<Cell<u32>>; 3],
    stage: Rc<Cell<u32>>,
    /// 真实 tab 条随控制条显隐（controls_visible 门控）
    controls_visible: bool,
}

impl Render for Host {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let in_mini = false;
        let labels = ["推荐", "漫剧", "真人"];
        let active = 0usize;
        v_flex()
            .relative()
            .size_full()
            .overflow_hidden()
            // —— 模拟画面层（先画）——
            .child(div().id("stage-video").absolute().size_full().on_click({
                let stage = Rc::clone(&self.stage);
                cx.listener(move |_, _, _, _| {
                    stage.set(stage.get() + 1);
                })
            }))
            // —— tab 条：真实同款（悬浮层总闸门控 + 顶部居中 + 组件 Button）——
            .children((!in_mini && self.controls_visible).then(|| {
                h_flex()
                    .absolute()
                    .top_3()
                    .left_0()
                    .right_0()
                    .justify_center()
                    .gap_2()
                    .children(labels.iter().enumerate().map(|(i, label)| {
                        let button = Button::new(format!("stage-tab-{label}"))
                            .label(*label)
                            .small();
                        let button = if i == active {
                            button.secondary()
                        } else {
                            button.ghost()
                        };
                        button.on_click({
                            let clicks = Rc::clone(&self.tab_clicks[i]);
                            cx.listener(move |_, _, _, cx| {
                                clicks.set(clicks.get() + 1);
                                cx.stop_propagation();
                            })
                        })
                    }))
            }))
            // —— 控制栏（后画、底部横条，同款 down/up 拦截）——
            .children(self.controls_visible.then(|| {
                h_flex()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(48.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
            }))
    }
}

#[gpui_kit::test]
async fn tab_click_reaches_on_click(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let tab_clicks = [
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
    ];
    let stage = Rc::new(Cell::new(0));
    let window = cx.add_window(|_, _cx| Host {
        tab_clicks: tab_clicks.clone(),
        stage: Rc::clone(&stage),
        controls_visible: true,
    });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    // 不猜几何：沿 tab 条所在高度带横扫点击，落在 tab 上的次数合计应 ≥ 3
    //（每只按钮至少一次）。
    vx.update(|window, cx| {
        for y in [18.0, 24.0, 30.0] {
            let mut x = 8.0;
            while x < 2000.0 {
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
                x += 8.0;
            }
        }
    });
    let tab_hits: u32 = tab_clicks.iter().map(|c| c.get()).sum();
    assert!(
        tab_hits >= 3,
        "横扫应命中全部三只 tab（合计 {tab_hits} 次）"
    );
}
