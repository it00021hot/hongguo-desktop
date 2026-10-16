//! 找剧页搜索联想的键盘派发断言：输入框获得焦点时，挂在其祖先上的冒泡
//! `on_key_down`（联想导航用）还能不能看到 ↑↓ / Enter / Esc。
//!
//! 背景：输入框自己在 "Input" key context 里绑了 MoveUp/MoveDown/Enter/
//! Escape（gpui-base 的绑定表），而 gpui 的按键派发顺序是「绑定（action）
//! 先、key listener 后」——所以这里的断言钉死两件事：
//!   1. 祖先的冒泡 listener 仍能收到这四个键（联想导航靠它工作）；
//!   2. 单行输入里 ↑↓ 不挪光标（输入框的 MoveUp/MoveDown 是空动作），
//!      Enter 的 action 只补发 InputEvent::PressEnter（找剧页不订阅它）。
//! 结论变了就是 gpui 升级改了派发语义，找剧页的键盘导航要一起复查。
//!
//! 需要 gpui 的 test-support（`screenshot` feature 已连带开启），运行方式：
//! `cargo test -p hongguo-gpui --features screenshot --test browse_suggest_keys`

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Entity, Focusable as _, Render, TestAppContext, VisualTestContext};

/// 最小宿主：搜索行（v_flex + on_key_down 捕获）套真实 Input。
struct Host {
    state: Entity<InputState>,
    seen: Rc<RefCell<Vec<String>>>,
}

impl Render for Host {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let seen = Rc::clone(&self.seen);
        v_flex()
            .id("search-row")
            .on_key_down(cx.listener(move |_, ev: &gpui_kit::KeyDownEvent, _, cx| {
                seen.borrow_mut().push(ev.keystroke.key.clone());
                cx.stop_propagation();
            }))
            .child(h_flex().child(Input::new(&self.state)))
    }
}

#[gpui_kit::test]
async fn ancestor_key_down_sees_keys_while_input_focused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let entered: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
    let seen_host = Rc::clone(&seen);
    let window = cx.add_window(move |window, cx| {
        let state = cx.new(|cx| InputState::new(window, cx));
        Host {
            state,
            seen: seen_host,
        }
    });
    let mut vx = VisualTestContext::from_window(window.into(), cx);
    let entered_sub = Rc::clone(&entered);
    vx.update(|window, cx| {
        let host = window.root::<Host>().flatten().expect("root is Host");
        let state = host.read(cx).state.clone();
        cx.subscribe(&state, move |_, event: &InputEvent, _| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                *entered_sub.borrow_mut() += 1;
            }
        })
        .detach();
        state.update(cx, |s, cx| {
            s.insert("测试", window, cx);
        });
        window.focus(&state.read(cx).focus_handle(cx), cx);
    });
    vx.simulate_keystrokes("down");
    vx.simulate_keystrokes("up");
    vx.simulate_keystrokes("enter");
    vx.simulate_keystrokes("escape");
    vx.update(|window, cx| {
        let host = window.root::<Host>().flatten().expect("root is Host");
        let text = host.read(cx).state.read(cx).value().to_string();
        assert_eq!(
            seen.borrow().as_slice(),
            ["down", "up", "enter", "escape"],
            "四个联想导航键都要冒泡到祖先 listener"
        );
        assert_eq!(*entered.borrow(), 1, "Enter 的 action 仍会补发 PressEnter");
        assert_eq!(text, "测试", "单行输入不该被这几个键改动文本");
    });
}

/// 输入框自己绑了 MoveUp/MoveDown（"Input" context）：单行输入里按 ↑↓
/// 到底动不动光标？联想导航依赖「按键还能被祖先拦截」，但也想知道输入框
/// 是否已经先把光标挪了（Tauri 版 preventDefault 后光标不动）。
#[gpui_kit::test]
async fn input_own_down_up_caret_effect(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let window = cx.add_window(move |window, cx| {
        let state = cx.new(|cx| InputState::new(window, cx));
        Host { state, seen }
    });
    let mut vx = VisualTestContext::from_window(window.into(), cx);
    let caret = |vx: &mut VisualTestContext| {
        vx.update(|window, cx| {
            let host = window.root::<Host>().flatten().expect("root is Host");
            host.read(cx).state.read(cx).selected_range()
        })
    };
    vx.update(|window, cx| {
        let host = window.root::<Host>().flatten().expect("root is Host");
        let state = host.read(cx).state.clone();
        state.update(cx, |s, cx| {
            s.insert("abc", window, cx);
        });
        window.focus(&state.read(cx).focus_handle(cx), cx);
    });
    assert_eq!(caret(&mut vx), 3..3, "插入后光标在末尾");
    vx.simulate_keystrokes("home");
    assert_eq!(caret(&mut vx), 0..0, "home 走输入框自己的 MoveHome 绑定");
    vx.simulate_keystrokes("down");
    assert_eq!(caret(&mut vx), 0..0, "↓ 不该挪动单行输入的光标");
    vx.simulate_keystrokes("up");
    assert_eq!(caret(&mut vx), 0..0, "↑ 不该挪动单行输入的光标");
}
