//! 回归：选集浮层「点外部关闭」（Tauri 同口径：锚点 = 入口按钮 + 浮层）。
//!
//! **事故**（用户实测「选集面板在点击其他控件时不会隐藏」）：原先「点外部
//! 关闭」靠舞台层那张透明捕获层，可它只铺在画面上，而控制条画在它**之上**
//! ——点控制条上的别的控件，捕获层压根收不到，浮层就一直挂着。
//!
//! **修法**：浮层本体挂窗口级的 `on_mouse_down_out`（capture 阶段、按命中盒
//! 判内外）：落在浮层外就收起，且**不吞**底下控件的点击（控制条按钮照常
//! 响应）。入口按钮本身要排除在外——那一下交给按钮的 on_click 收起；捕获
//! 层若先把它关了，按钮的 on_click 又会把它打开，面板关不掉还来回横跳。
//!
//! 本壳是 bin-only（`[[bin]] test = false`），源码内放不了单测，所以分两半：
//! - **行为**：用 gpui 测试上下文复刻真实结构（入口 wrapper + deferred 浮层 +
//!   舞台捕获层 + 控制条兄弟控件），验证这套机制真的「关得掉、又不吞点击」；
//! - **接线**：源码守卫盯住真壳代码确实用了这套机制（复刻通过、真壳照旧的
//!   话守卫会红）。
//!
//! 运行：`cargo test -p hongguo-gpui --features screenshot --test picker_dismiss`

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    Bounds, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput,
    Point, Render, div, px,
};

/// 复刻真实壳的选集浮层结构。
///
/// 层序与真壳一致：画面（绝对定位铺满）→ 控制条（绝对定位贴底，画在画面
/// 之上）→ 控制条里一只兄弟控件（静音钮）与选集入口 wrapper（wrapper 里挂
/// deferred 浮层）。**控制条在舞台捕获层之上**正是事故的成因，布局必须照抄。
struct PickerHost {
    /// 浮层开着（真壳：控制条「选集」按钮 toggle 出来的那个面板）。
    panel_open: bool,
    /// 入口按钮的矩形哨兵（真壳同款：prepaint 采集，点外部关闭时排除它）。
    entry_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// 控制条兄弟控件（静音钮）的矩形哨兵。
    mute_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// 浮层里一集的矩形哨兵（点它 = 选集，浮层不该关）。
    episode_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    mute_clicks: Rc<Cell<u32>>,
    entry_clicks: Rc<Cell<u32>>,
    episode_clicks: Rc<Cell<u32>>,
}

impl Render for PickerHost {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let entry_bounds = Rc::clone(&self.entry_bounds);
        let mute_bounds = Rc::clone(&self.mute_bounds);
        div()
            .size_full()
            .relative()
            // 画面：浮层开着时铺捕获层（点画面 = 关浮层，且不落到画面上，
            // 免得点画面变成暂停）。
            .child(
                div()
                    .id("stage")
                    .absolute()
                    .size_full()
                    .children(self.panel_open.then(|| {
                        div()
                            .absolute()
                            .size_full()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.panel_open = false;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .into_any_element()
                    })),
            )
            // 控制条（画在画面之上——这正是舞台捕获层收不到它点击的原因）。
            .child(
                h_flex()
                    // prepaint 哨兵是 Div 的方法：必须在 .id() 变 Stateful 之前挂
                    .on_children_prepainted(move |children, _, _| {
                        // 第一个子层是静音钮
                        if let Some(b) = children.first() {
                            mute_bounds.set(Some(*b));
                        }
                    })
                    .id("controls")
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(40.0))
                    .px_3()
                    .gap_3()
                    // 真壳同款纪律：控制条拦下鼠标下传（gpui 对重叠 bounds 的
                    // 兄弟分支都会派发事件，不拦的话点按钮会同时触发舞台的
                    // 暂停）。**这条拦截正是舞台捕获层收不到控制条点击的
                    // 原因**——「点外部关闭」不能指望那张捕获层。
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .child(
                        div()
                            .id("stage-mute")
                            .w(px(24.0))
                            .h(px(24.0))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mute_clicks.set(this.mute_clicks.get() + 1);
                                cx.notify();
                            })),
                    )
                    // 垫块：把选集入口推到窗口中间，浮层（贴按钮右缘）才落在
                    // 屏内（真壳里入口在控制条右端，同理）。
                    .child(div().w(px(400.0)))
                    .child(
                        div()
                            .on_children_prepainted(move |children, _, _| {
                                // 第一个子层是入口按钮（浮层是 deferred 子层，排后面）
                                if let Some(b) = children.first() {
                                    entry_bounds.set(Some(*b));
                                }
                            })
                            .id("stage-picker-wrap")
                            .relative()
                            .child(div().id("stage-picker").w(px(80.0)).h(px(24.0)).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.entry_clicks.set(this.entry_clicks.get() + 1);
                                    this.panel_open = !this.panel_open;
                                    cx.notify();
                                }),
                            ))
                            .children(self.panel_open.then(|| {
                                gpui_kit::deferred(self.render_panel(cx)).with_priority(1)
                            })),
                    ),
            )
    }
}

impl PickerHost {
    /// 选集浮层本体（真壳同款：入口 wrapper 里的 deferred 子层）。
    fn render_panel(&self, cx: &gpui_kit::Context<Self>) -> gpui_kit::AnyElement {
        let episode_bounds = Rc::clone(&self.episode_bounds);
        v_flex()
            // prepaint 哨兵是 Div 的方法：必须在 .id() 变 Stateful 之前挂
            .on_children_prepainted(move |children, _, _| {
                if let Some(b) = children.first() {
                    episode_bounds.set(Some(*b));
                }
            })
            .id("picker-panel")
            .absolute()
            .right_0()
            .bottom(gpui_kit::relative(1.0))
            .w(px(200.0))
            .h(px(120.0))
            .bg(gpui_kit::rgb(0x222222))
            // 点外部关闭：窗口级监听（capture 阶段、按命中盒判内外），不吞
            // 底下控件的点击。
            .on_mouse_down_out(cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                // 入口按钮本身不算「点外部」：那一下交给按钮的 on_click 收起
                if let Some(b) = this.entry_bounds.get()
                    && b.contains(&ev.position)
                {
                    return;
                }
                this.panel_open = false;
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .child(
                div()
                    .id("picker-ep-1")
                    .w(px(40.0))
                    .h(px(24.0))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.episode_clicks.set(this.episode_clicks.get() + 1);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

/// 在指定位置做一次完整点击（移动 → 按下 → 抬起）。
fn click(vx: &mut gpui_kit::VisualTestContext, pos: Point<Pixels>) {
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

/// 矩形中心点。
fn center(b: Bounds<Pixels>) -> Point<Pixels> {
    Point::new(
        b.origin.x + b.size.width / 2.0,
        b.origin.y + b.size.height / 2.0,
    )
}

/// 点控制条上的别的控件：浮层要收起，且**那一击不许被吞**。
///
/// 吞掉的话，用户点静音/全屏等按钮会毫无反应——比浮层不关更糟。
#[gpui_kit::test]
async fn clicking_other_control_dismisses_panel_without_swallowing_click(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let entry_bounds = Rc::new(Cell::new(None));
    let mute_bounds = Rc::new(Cell::new(None));
    let episode_bounds = Rc::new(Cell::new(None));
    let mute_clicks = Rc::new(Cell::new(0));
    let entry_clicks = Rc::new(Cell::new(0));
    let episode_clicks = Rc::new(Cell::new(0));
    let (view, vx) = cx.add_window_view({
        let entry_bounds = entry_bounds.clone();
        let mute_bounds = mute_bounds.clone();
        let episode_bounds = episode_bounds.clone();
        let mute_clicks = mute_clicks.clone();
        let entry_clicks = entry_clicks.clone();
        let episode_clicks = episode_clicks.clone();
        move |_, _cx| PickerHost {
            panel_open: true,
            entry_bounds,
            mute_bounds,
            episode_bounds,
            mute_clicks,
            entry_clicks,
            episode_clicks,
        }
    });
    // 先画一帧：哨兵要采到矩形
    vx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    vx.run_until_parked();

    let mute = mute_bounds.get().expect("兄弟控件矩形应已采集");
    click(vx, center(mute));

    assert_eq!(
        mute_clicks.get(),
        1,
        "控制条兄弟控件的点击不该被吞（浮层收起不该吃掉底下控件的动作）"
    );
    assert!(
        !vx.read(|app| view.read(app).panel_open),
        "点别的控件时选集浮层应收起"
    );
}

/// 点入口按钮：收起即可，且**不许又弹回来**。
///
/// 「捕获层先关 → 按钮 on_click 再开」的双翻会让面板关不掉还反复横跳。
#[gpui_kit::test]
async fn clicking_entry_button_closes_panel_without_reopening(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let entry_bounds = Rc::new(Cell::new(None));
    let mute_bounds = Rc::new(Cell::new(None));
    let episode_bounds = Rc::new(Cell::new(None));
    let mute_clicks = Rc::new(Cell::new(0));
    let entry_clicks = Rc::new(Cell::new(0));
    let episode_clicks = Rc::new(Cell::new(0));
    let (view, vx) = cx.add_window_view({
        let entry_bounds = entry_bounds.clone();
        let mute_bounds = mute_bounds.clone();
        let episode_bounds = episode_bounds.clone();
        let mute_clicks = mute_clicks.clone();
        let entry_clicks = entry_clicks.clone();
        let episode_clicks = episode_clicks.clone();
        move |_, _cx| PickerHost {
            panel_open: true,
            entry_bounds,
            mute_bounds,
            episode_bounds,
            mute_clicks,
            entry_clicks,
            episode_clicks,
        }
    });
    vx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    vx.run_until_parked();

    let entry = entry_bounds.get().expect("入口按钮矩形应已采集");
    click(vx, center(entry));

    assert_eq!(entry_clicks.get(), 1, "入口按钮应照常响应点击");
    assert!(
        !vx.read(|app| view.read(app).panel_open),
        "点入口按钮应收起浮层（不许被 on_click 又打开）"
    );
}

/// 点浮层内部：浮层留着（连选连跳），选集照常生效。
#[gpui_kit::test]
async fn clicking_inside_panel_keeps_it_open(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let entry_bounds = Rc::new(Cell::new(None));
    let mute_bounds = Rc::new(Cell::new(None));
    let episode_bounds = Rc::new(Cell::new(None));
    let mute_clicks = Rc::new(Cell::new(0));
    let entry_clicks = Rc::new(Cell::new(0));
    let episode_clicks = Rc::new(Cell::new(0));
    let (view, vx) = cx.add_window_view({
        let entry_bounds = entry_bounds.clone();
        let mute_bounds = mute_bounds.clone();
        let episode_bounds = episode_bounds.clone();
        let mute_clicks = mute_clicks.clone();
        let entry_clicks = entry_clicks.clone();
        let episode_clicks = episode_clicks.clone();
        move |_, _cx| PickerHost {
            panel_open: true,
            entry_bounds,
            mute_bounds,
            episode_bounds,
            mute_clicks,
            entry_clicks,
            episode_clicks,
        }
    });
    vx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    vx.run_until_parked();

    let episode = episode_bounds.get().expect("集号格矩形应已采集");
    click(vx, center(episode));

    assert_eq!(episode_clicks.get(), 1, "浮层内的集号格应可点");
    assert!(
        vx.read(|app| view.read(app).panel_open),
        "点浮层内部不该收起浮层（连选连跳）"
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

/// 选集浮层必须挂「点外部关闭」，且排除入口按钮。
#[test]
fn shell_picker_panel_dismisses_on_outside_click() {
    let picker = source("home/picker.rs");
    assert!(
        picker.contains(".on_mouse_down_out("),
        "选集浮层必须挂 on_mouse_down_out（点外部关闭）——控制条画在舞台捕获层之上，只靠那张捕获层收不到"
    );
    assert!(
        picker.contains("picker_entry_bounds"),
        "点外部关闭必须排除入口按钮本身（否则捕获层先关、按钮 on_click 又开，来回横跳）"
    );
    let menus = source("home/controls/menus.rs");
    assert!(
        menus.contains("picker_entry_bounds"),
        "入口按钮的矩形哨兵必须在选集入口 wrapper 上采集（menus.rs）"
    );
    assert!(
        menus.contains("on_children_prepainted"),
        "选集入口 wrapper 要挂 prepaint 哨兵采集按钮矩形"
    );
}
