//! 评论面板输入框「点不出光标」的行为复现：按真实舞台的层序与结构搭最小
//! 模型（画面点击层 → 控制条热区 → 评论面板），点真实 Input 组件（含
//! `SkinnedInput` 皮肤链），裁决按下后焦点是否落到输入框、点击是否漏到
//! 画面层。
//!
//! 需要 gpui 的 test-support（`screenshot` feature 已连带开启），运行方式：
//! `cargo test -p hongguo-gpui --features screenshot --test comment_input_click`

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::Theme;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    Entity, Focusable as _, MouseButton, PlatformInput, Point, Render, RenderOnce, div, px,
};

/// 复刻壳里的 `SkinnedInput`（home/controls/mod.rs）：渲染期临时把光标/
/// 占位符/选区/编辑器底色四个 token 换成播放器皮肤值，`Input::render`
/// 同步读走后再还原——不保真这条链，复现就不算数。
#[derive(gpui_kit::IntoElement)]
struct SkinnedInput {
    inner: Input,
}

impl RenderOnce for SkinnedInput {
    fn render(
        self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    ) -> impl gpui_kit::IntoElement {
        let (caret, muted, selection, editor_bg) = {
            let theme = Theme::global_mut(cx);
            let saved = (
                theme.caret,
                theme.muted_foreground,
                theme.selection,
                theme.highlight_theme.style.editor_background,
            );
            theme.caret = gpui_kit::rgba(0xffffffff).into();
            theme.muted_foreground = gpui_kit::rgba(0xffffff99).into();
            theme.selection = gpui_kit::rgba(0xffffff40).into();
            std::sync::Arc::make_mut(&mut theme.highlight_theme)
                .style
                .editor_background = Some(gpui_kit::rgba(0x00000000).into());
            saved
        };
        let element = self.inner.render(window, cx).into_any_element();
        let theme = Theme::global_mut(cx);
        theme.caret = caret;
        theme.muted_foreground = muted;
        theme.selection = selection;
        std::sync::Arc::make_mut(&mut theme.highlight_theme)
            .style
            .editor_background = editor_bg;
        element
    }
}

struct Host {
    input: Entity<InputState>,
    /// 画面层收到的点击数（面板的 stop 应拦到 0）
    stage_clicks: Rc<Cell<u32>>,
    /// 真实壳的舞台焦点锚：启动种子聚焦 + video 点击回收焦点
    stage_focus: gpui_kit::FocusHandle,
    /// 模拟播放期持续重渲染（每帧 HomeView 都会重建整棵树）
    keep_rendering: Rc<Cell<bool>>,
}

impl Render for Host {
    fn render(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // 真实壳 view.rs 的焦点种子：首帧把焦点锚在舞台（只播一次）
        if !self.stage_focus.is_focused(window) && !self.stage_focus.contains_focused(window, cx) {
            window.focus(&self.stage_focus, cx);
        }
        // 播放期 HomeView 每帧重建：这里用 16ms 定时自触发模拟同一条件
        if self.keep_rendering.get() {
            cx.notify();
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.keep_rendering.get() {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let stage_clicks = Rc::clone(&self.stage_clicks);
        v_flex()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(gpui_kit::rgb(0x000000))
            .key_context("HomeStage")
            .track_focus(&self.stage_focus)
            // 画面点击层（点画面 = 暂停；先画）
            .child(
                div()
                    .id("stage-video")
                    .absolute()
                    .size_full()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        stage_clicks.set(stage_clicks.get() + 1);
                        window.focus(&this.stage_focus, cx);
                    })),
            )
            // 控制条热区（后于画面、先于面板；root 带 down/up 拦截 +
            // pt_10 悬停热区，几何照抄 controls/bar.rs）
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
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .h(px(4.0))
                            .rounded(gpui_kit::px(2.0))
                            .bg(gpui_kit::rgba(0xffffff33)),
                    )
                    .child(
                        h_flex()
                            .h(px(28.0))
                            .child(div().size_8().rounded_full().bg(gpui_kit::rgba(0xffffff33))),
                    ),
            )
            // 评论面板（comment_view/mod.rs 1:1：几何、皮肤、两层拦截）
            .child(
                v_flex()
                    .id("comment-panel")
                    .absolute()
                    .right_0()
                    .bottom(px(56.0))
                    .h(gpui_kit::relative(0.68))
                    .w(px(380.0))
                    .bg(gpui_kit::rgba(0x0a0a0af2))
                    .rounded_tl(px(12.0))
                    .border_t_1()
                    .border_l_1()
                    .border_color(gpui_kit::rgba(0xffffff1a))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .h(px(48.0))
                            .border_b_1()
                            .border_color(gpui_kit::rgba(0xffffff1a)),
                    )
                    .child(
                        v_flex()
                            .id("comments-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px_4(),
                    )
                    .child(
                        // 发送区（composer.rs 1:1 的输入行）
                        v_flex()
                            .px_4()
                            .py_3()
                            .gap_2()
                            .relative()
                            .border_t_1()
                            .border_color(gpui_kit::rgba(0xffffff1a))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().w(px(20.0)).h(px(20.0)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .h(px(36.0))
                                            .flex()
                                            .items_center()
                                            .rounded(px(6.0))
                                            .bg(gpui_kit::rgba(0x262626cc))
                                            .px_3()
                                            // 二分开关：环境变量置 1 时走裸 Input
                                            //（绕过 SkinnedInput 皮肤链）
                                            .child(SkinnedInput {
                                                inner: Input::new(&self.input)
                                                    .appearance(false)
                                                    .h_full()
                                                    .flex_1()
                                                    .text_sm()
                                                    .text_color(gpui_kit::rgba(0xffffffff)),
                                            }),
                                    )
                                    .child(div().w(px(20.0)).h(px(20.0)))
                                    .child(div().w(px(56.0)).h(px(36.0))),
                            ),
                    ),
            )
    }
}

#[gpui_kit::test]
async fn click_focuses_comment_input(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let stage_clicks = Rc::new(Cell::new(0));
    let keep_rendering = Rc::new(Cell::new(false));
    let window = cx.add_window(|window, cx| {
        let input = cx.new(|cx| InputState::new(window, cx));
        Host {
            input,
            stage_clicks: Rc::clone(&stage_clicks),
            stage_focus: cx.focus_handle(),
            keep_rendering: Rc::clone(&keep_rendering),
        }
    });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let (w, h) = vx.update(|window, _| {
        let size = window.viewport_size();
        (size.width.as_f32(), size.height.as_f32())
    });
    // 播放期持续重渲染 + 首帧种子聚焦（真实壳同款初始条件）
    keep_rendering.set(true);
    vx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    // 输入行中心：面板底边 = 窗高 - 56，其上是 py_3(12) + 输入行 h 36
    let pos = Point::new(px(w - 220.0), px(h - 86.0));
    vx.update(|window, cx| {
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
    });
    keep_rendering.set(false);
    vx.update(|window, cx| {
        let host = window.root::<Host>().flatten().expect("root view is Host");
        let focused = host
            .read(cx)
            .input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        assert!(
            focused,
            "点击评论输入框后焦点必须落在输入框上（无焦点 = 无光标）"
        );
    });
    assert_eq!(
        stage_clicks.get(),
        0,
        "面板里的点击不许漏到画面层（漏了会顺带暂停视频）"
    );
}
