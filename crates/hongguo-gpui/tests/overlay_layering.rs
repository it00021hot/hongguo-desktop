//! 回归：控制栏向上弹浮层的层级。gpui 没有 z-index，层级 = 绘制顺序——浮层
//! 不 `deferred` 会被后画的兄弟元素盖住；且鼠标事件按命中盒几何派发、重叠
//! 兄弟都会收到事件，层级只能靠绘制顺序解决。分两半：行为（deferred 后
//! 交互不丢）+ 接线守卫（三个控制栏浮层都用了 deferred）。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Point, Render, div,
    px,
};

/// 浮层与「后画的兄弟」盖在同一块地方。
struct Host {
    /// 浮层是否 deferred（真壳口径 = true）。
    defer_popup: bool,
    popup_hits: Rc<Cell<u32>>,
    sibling_hits: Rc<Cell<u32>>,
    /// 浮层的 hover 次数（音量浮层的开合完全靠它）。
    popup_hovers: Rc<Cell<u32>>,
}

impl Render for Host {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let popup = v_flex()
            .id("popup")
            .absolute()
            .left(px(100.0))
            .top(px(100.0))
            .w(px(60.0))
            .h(px(120.0))
            .bg(gpui_kit::rgb(0x222222))
            .on_hover(cx.listener(|this, _: &bool, _, cx| {
                this.popup_hovers.set(this.popup_hovers.get() + 1);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.popup_hits.set(this.popup_hits.get() + 1);
                    cx.notify();
                }),
            )
            .into_any_element();
        div()
            .size_full()
            .relative()
            // 兄弟：普通绘制序里**排在浮层后面**（真壳里就是评论面板这类
            // 后画的元素），盖住浮层同一块地方。
            .child(
                div()
                    .id("sibling")
                    .absolute()
                    .left(px(100.0))
                    .top(px(100.0))
                    .w(px(60.0))
                    .h(px(120.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.sibling_hits.set(this.sibling_hits.get() + 1);
                            cx.notify();
                        }),
                    ),
            )
            .child(if self.defer_popup {
                gpui_kit::deferred(popup)
                    .with_priority(1)
                    .into_any_element()
            } else {
                popup.into_any_element()
            })
    }
}

/// 测试脚手架：上下文 + 三处记账位。
struct Fixture<'a> {
    vx: &'a mut gpui_kit::VisualTestContext,
    popup_hits: Rc<Cell<u32>>,
    sibling_hits: Rc<Cell<u32>>,
    popup_hovers: Rc<Cell<u32>>,
}

/// 造窗口。
fn host(cx: &mut gpui_kit::TestAppContext, defer_popup: bool) -> Fixture<'_> {
    let popup_hits = Rc::new(Cell::new(0));
    let sibling_hits = Rc::new(Cell::new(0));
    let popup_hovers = Rc::new(Cell::new(0));
    let (_view, vx) = cx.add_window_view({
        let popup_hits = popup_hits.clone();
        let sibling_hits = sibling_hits.clone();
        let popup_hovers = popup_hovers.clone();
        move |_, _cx| Host {
            defer_popup,
            popup_hits,
            sibling_hits,
            popup_hovers,
        }
    });
    Fixture {
        vx,
        popup_hits,
        sibling_hits,
        popup_hovers,
    }
}

/// 悬停到浮层中心。
fn hover(vx: &mut gpui_kit::VisualTestContext, pos: Point<gpui_kit::Pixels>) {
    vx.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: pos,
                pressed_button: None,
                modifiers: Default::default(),
            }),
            cx,
        );
    });
    vx.run_until_parked();
}

/// 在浮层中心按下。
fn press(vx: &mut gpui_kit::VisualTestContext, pos: Point<gpui_kit::Pixels>) {
    vx.update(|window, cx| {
        for input in [
            PlatformInput::MouseMove(MouseMoveEvent {
                position: pos,
                pressed_button: None,
                modifiers: Default::default(),
            }),
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: pos,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }),
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: pos,
                modifiers: Default::default(),
                click_count: 1,
            }),
        ] {
            window.dispatch_event(input, cx);
        }
    });
    vx.run_until_parked();
}

/// `deferred` 之后浮层的交互不能丢：hover 与命中照常到达。
///
/// 音量浮层的开合完全靠 hover（`volume_hovered || volume_popup_hovered`），
/// 浮层一旦收不到 hover 就会当场消失——这条守住 deferred 化的副作用面。
#[gpui_kit::test]
async fn deferred_popup_keeps_hover_and_clicks(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let fx = host(cx, true);
    fx.vx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    fx.vx.run_until_parked();

    let pos = Point::new(px(130.0), px(160.0));
    hover(fx.vx, pos);
    assert!(
        fx.popup_hovers.get() >= 1,
        "deferred 浮层仍要收到 hover（音量浮层的开合完全靠它）"
    );

    press(fx.vx, pos);
    assert_eq!(fx.popup_hits.get(), 1, "deferred 浮层仍要收到命中");
}

/// gpui 的鼠标事件按命中盒**几何**派发：重叠的兄弟都会收到。
///
/// 这条钉住「层级只能靠绘制顺序（deferred）解决」——命中顺序解决不了：压不
/// 住别人的同时也会被别人压。也解释了控制条根节点为什么必须
/// `stop_propagation`（点进度条/按钮别同时触发舞台的点击暂停）。
#[gpui_kit::test]
async fn overlapping_siblings_both_receive_mouse_down(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let fx = host(cx, true);
    fx.vx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    fx.vx.run_until_parked();

    press(fx.vx, Point::new(px(130.0), px(160.0)));
    assert_eq!(fx.popup_hits.get(), 1, "重叠区里浮层要收到命中");
    assert_eq!(
        fx.sibling_hits.get(),
        1,
        "gpui 按命中盒几何派发：重叠的兄弟也会收到（层级得靠绘制顺序，不能靠命中顺序）"
    );
}

// —— 真壳接线守卫 ——

/// 读壳内某个源码文件（去注释后的代码文本）。
fn source(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到源码 {rel}：{e}"));
    text.lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 控制栏向上弹的浮层必须 `deferred`（层级 = 绘制顺序）。
///
/// 漏一个，那个浮层就会被后画的兄弟元素盖住（浮层只剩半截）。
#[test]
fn control_bar_popups_are_deferred() {
    // 音量竖条浮层
    let volume = source("home/controls/volume.rs");
    let at = volume
        .find("volume-popup")
        .expect("volume.rs 里要有 volume-popup");
    assert!(
        volume[at..].contains("deferred("),
        "音量浮层必须用 deferred 包裹（控制栏浮层的层级靠它，否则被后画的兄弟盖住）"
    );

    // 弹幕设置面板
    let menus = source("home/controls/menus.rs");
    let at = menus
        .find("dm-settings-panel")
        .expect("menus.rs 里要有 dm-settings-panel");
    assert!(
        menus[at..].contains("deferred("),
        "弹幕设置面板必须用 deferred 包裹（否则被后画的兄弟盖住）"
    );

    // 选集浮层（面板本体在 picker.rs，deferred 包裹在入口处 menus.rs）
    assert!(
        menus.contains("deferred(self.render_picker"),
        "选集浮层必须用 deferred 包裹（否则被后画的兄弟盖住）"
    );
}
