//! 回归：音量弹层「按住拖动」（用户实测「声音控件没法鼠标按住拖动调整音量」）。
//!
//! **根因**：弹层是 **hover 驱动**的（按钮 hover ∥ 弹层 hover）。gpui 进入
//! 拖拽捕获后不再按命中盒判 hover，弹层的 hover 位当场掉 → 弹层收起 → 滑杆
//! 从元素树里消失 → `drag_move` 再也送不到它，音量停在按下处。
//!
//! **修法**：拖动期间把弹层**钉住**（`volume_dragging` 位并进弹层开合裁决），
//! 松手再放开。
//!
//! 本壳是 bin-only（`[[bin]] test = false`），源码内放不了单测，所以分两半：
//! - **行为**：用 gpui 测试上下文复刻真实结构（按钮 hover 开弹层 / 弹层内竖向
//!   滑杆 on_drag），并带一个**负对照**（修法前的纯 hover 裁决）——负对照必须
//!   拖不动，正对照必须拖得动，两头夹住这条回归；
//! - **接线**：源码守卫盯住真壳代码确实把「拖动中」算进弹层开合裁决。
//!
//! 运行：`cargo test -p hongguo-gpui --features screenshot --test volume_drag`

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    Bounds, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput,
    Point, Render, div, px,
};

/// 滑杆的拖拽载荷（`on_drag` / `on_drag_move` 按类型配对）。
struct VolumeDrag;

struct Ghost;

impl Render for Ghost {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        _cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        div()
    }
}

/// 复刻真实壳的音量弹层结构。
///
/// `pin_on_drag` 是唯一变量：`true` = 修法后（拖动期间钉住弹层），
/// `false` = 修法前（纯 hover 裁决）。
struct VolumeHost {
    pin_on_drag: bool,
    btn_hovered: bool,
    popup_hovered: bool,
    /// 滑杆正被按住（按下置位、松手清位）。
    dragging: bool,
    slider_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    drag_moves: Rc<Cell<u32>>,
    value: Rc<Cell<f32>>,
}

impl VolumeHost {
    /// 弹层开合裁决（真壳同款：按钮 hover ∥ 弹层 hover ∥ 拖动中）。
    fn popup_open(&self) -> bool {
        self.btn_hovered || self.popup_hovered || (self.pin_on_drag && self.dragging)
    }
}

impl Render for VolumeHost {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let slider_bounds = Rc::clone(&self.slider_bounds);
        let drag_moves = Rc::clone(&self.drag_moves);
        div()
            .size_full()
            .relative()
            // 音量按钮（hover 开弹层）
            .child(
                div()
                    .id("stage-mute")
                    .absolute()
                    .left(px(100.0))
                    .top(px(400.0))
                    .size(px(24.0))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.btn_hovered = *hovered;
                        cx.notify();
                    })),
            )
            // 弹层（自身 hover 也算开着）
            .children(self.popup_open().then(|| {
                // 监听器要 'static：把记账位按值搬进闭包里
                let slider_bounds = Rc::clone(&slider_bounds);
                let drag_moves = Rc::clone(&drag_moves);
                v_flex()
                    .id("volume-popup")
                    .absolute()
                    .left(px(70.0))
                    .top(px(280.0))
                    .w(px(60.0))
                    .h(px(110.0))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.popup_hovered = *hovered;
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .child(
                        div()
                            // prepaint 哨兵是 Div 的方法：必须在 .id() 变 Stateful 之前挂
                            .on_children_prepainted(move |children, _, _| {
                                if let Some(b) = children.first() {
                                    slider_bounds.set(Some(*b));
                                }
                            })
                            .id("volume-slider")
                            .absolute()
                            .left(px(18.0))
                            .top(px(7.0))
                            .w(px(24.0))
                            .h(px(96.0))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    // 按住即进入拖动态（真壳同款：弹层钉住）
                                    this.dragging = true;
                                    this.value.set(0.5);
                                    cx.notify();
                                }),
                            )
                            .on_drag(VolumeDrag, |_, _, _, cx| cx.new(|_| Ghost))
                            .on_drag_move::<VolumeDrag>(cx.listener(
                                move |this, _: &gpui_kit::DragMoveEvent<VolumeDrag>, _, cx| {
                                    drag_moves.set(drag_moves.get() + 1);
                                    this.value.set(0.9);
                                    cx.notify();
                                },
                            ))
                            // 松手收口：滑杆内/外都要算
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                                    this.dragging = false;
                                    cx.notify();
                                }),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                                    this.dragging = false;
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(10.0))
                                    .w(px(4.0)),
                            ),
                    )
            }))
    }
}

/// 悬停到某点。
fn hover(vx: &mut gpui_kit::VisualTestContext, pos: Point<Pixels>) {
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

/// 按下并按住（不松手）。
fn press(vx: &mut gpui_kit::VisualTestContext, pos: Point<Pixels>) {
    vx.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: pos,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    });
    vx.run_until_parked();
}

/// 按住状态下移动（拖动）。
fn drag_to(vx: &mut gpui_kit::VisualTestContext, pos: Point<Pixels>) {
    vx.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: pos,
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            }),
            cx,
        );
    });
    vx.run_until_parked();
}

/// 松手。
fn release(vx: &mut gpui_kit::VisualTestContext, pos: Point<Pixels>) {
    vx.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: pos,
                modifiers: Default::default(),
                click_count: 1,
            }),
            cx,
        );
    });
    vx.run_until_parked();
}

/// 矩形中心点。
fn center(b: Bounds<Pixels>) -> Point<Pixels> {
    Point::new(
        b.origin.x + b.size.width / 2.0,
        b.origin.y + b.size.height / 2.0,
    )
}

/// 测试脚手架：视图 + 上下文 + 记账位。
struct VolumeFixture<'a> {
    view: gpui_kit::Entity<VolumeHost>,
    vx: &'a mut gpui_kit::VisualTestContext,
    slider_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    drag_moves: Rc<Cell<u32>>,
}

/// 造一个窗口与配套的记账位。
fn host(cx: &mut gpui_kit::TestAppContext, pin_on_drag: bool) -> VolumeFixture<'_> {
    let slider_bounds = Rc::new(Cell::new(None));
    let drag_moves = Rc::new(Cell::new(0));
    let (view, vx) = cx.add_window_view({
        let slider_bounds = slider_bounds.clone();
        let drag_moves = drag_moves.clone();
        move |_, _cx| VolumeHost {
            pin_on_drag,
            btn_hovered: false,
            popup_hovered: false,
            dragging: false,
            slider_bounds,
            drag_moves,
            value: Rc::new(Cell::new(0.0)),
        }
    });
    VolumeFixture {
        view,
        vx,
        slider_bounds,
        drag_moves,
    }
}

/// 负对照：修法前（纯 hover 裁决）按住拖动会丢掉这次拖动。
///
/// 弹层在拖拽捕获期间收起 → 滑杆从元素树里消失 → drag_move 送不到。
/// 这条钉住「回归确实存在」——正对照通过而负对照也通过，说明断言本身没咬到
/// 点上。
#[gpui_kit::test]
async fn hover_only_popup_loses_the_drag(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let fx = host(cx, false);
    fx.vx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    fx.vx.run_until_parked();

    // 悬停按钮 → 弹层打开
    hover(fx.vx, Point::new(px(112.0), px(412.0)));
    // 移到滑杆上
    let slider = fx.slider_bounds.get().expect("滑杆矩形应已采集");
    let x = center(slider).x;
    let y = center(slider).y;
    hover(fx.vx, Point::new(x, y));
    // 按下并拖动（两段移动：gpui 的拖拽在**第一段**移动里才起拖，
    // drag_move 从第二段开始派发）
    press(fx.vx, Point::new(x, y));
    drag_to(fx.vx, Point::new(x, y - px(30.0)));
    drag_to(fx.vx, Point::new(x, y - px(40.0)));

    assert_eq!(
        fx.drag_moves.get(),
        0,
        "负对照：纯 hover 裁决的弹层在拖动中会收起，drag_move 送不到滑杆"
    );
}

/// 正对照：钉住弹层后，按住拖动能改音量。
#[gpui_kit::test]
async fn pinned_popup_keeps_the_drag_alive(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let fx = host(cx, true);
    fx.vx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    fx.vx.run_until_parked();

    // 悬停按钮 → 弹层打开
    hover(fx.vx, Point::new(px(112.0), px(412.0)));
    // 移到滑杆上
    let slider = fx.slider_bounds.get().expect("滑杆矩形应已采集");
    let x = center(slider).x;
    let y = center(slider).y;
    hover(fx.vx, Point::new(x, y));
    // 按下并拖动（两段移动：gpui 的拖拽在第一段移动里才起拖，drag_move 从
    // 第二段开始派发）
    press(fx.vx, Point::new(x, y));
    drag_to(fx.vx, Point::new(x, y - px(30.0)));
    drag_to(fx.vx, Point::new(x, y - px(40.0)));

    assert!(
        fx.drag_moves.get() >= 1,
        "钉住弹层后按住拖动应收到 drag_move（音量跟着走）"
    );
    assert!(
        fx.vx.read(|app| fx.view.read(app).popup_open()),
        "拖动期间弹层不许收（收了就滑杆消失，拖不动）"
    );

    // 松手 → 拖动态结束
    release(fx.vx, Point::new(x, y - px(30.0)));
    assert!(
        !fx.vx.read(|app| fx.view.read(app).dragging),
        "松手后拖动态应结束"
    );
}

// —— 真壳接线守卫（复刻通过不代表真壳接了同一套机制）——

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

/// 音量弹层的开合裁决必须把「拖动中」算进去。
#[test]
fn shell_volume_popup_is_pinned_while_dragging() {
    let src = source("home/controls/volume.rs");
    let open_line = src
        .lines()
        .find(|line| line.contains("let open ="))
        .expect("volume.rs 里要有弹层开合裁决");
    assert!(
        open_line.contains("volume_dragging"),
        "弹层开合裁决必须算上拖动态（hover 位在拖拽捕获期间会掉，不钉住就收）"
    );
    assert!(
        src.contains("set_volume_dragging(true"),
        "滑杆按下必须进入拖动态"
    );
    assert!(
        src.contains("set_volume_dragging(false"),
        "滑杆松手（轨道内/外都要）必须退出拖动态"
    );
}
