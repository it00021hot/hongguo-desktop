//! 渲染期主题皮肤补丁的安全性断言：`Input` 在 `RenderOnce::render` 里把
//! 光标/占位符/选区颜色从 `cx.theme()` 拷进 editor style，播放器 chrome 要
//! 固定深色皮肤就只能在这个窗口里临时改主题 token。这里验证两件事：
//! 1. 渲染期 `Theme::global_mut` 不会撞借用 panic（整棵树能画出来）；
//! 2. 渲染结束后主题被原样还原（不会污染其它页面）。
//!
//! 运行：`cargo test -p hongguo-gpui --features screenshot --test skin_input`

use gpui_kit::component::Theme;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{AppContext as _, Entity, IntoElement, Render, RenderOnce, TestAppContext, Window};

/// 复刻壳里的 `SkinnedInput`：改 token → 渲染 → 还原。
#[derive(gpui_kit::IntoElement)]
struct Skin {
    inner: Input,
}

impl RenderOnce for Skin {
    fn render(self, window: &mut Window, cx: &mut gpui_kit::App) -> impl IntoElement {
        let saved = {
            let theme = Theme::global_mut(cx);
            let saved = theme.caret;
            theme.caret = gpui_kit::rgba(0xff0000ff).into();
            saved
        };
        let element = self.inner.render(window, cx).into_any_element();
        Theme::global_mut(cx).caret = saved;
        element
    }
}

struct Host {
    state: Entity<InputState>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        Skin {
            inner: Input::new(&self.state),
        }
    }
}

#[gpui_kit::test]
async fn render_time_theme_patch_is_safe_and_restored(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let before = cx.update(|cx| Theme::global(cx).caret);
    let window = cx.add_window(|window, cx| {
        let state = cx.new(|cx| InputState::new(window, cx));
        Host { state }
    });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    // 画两轮：第一轮建树，第二轮走热路径（主题已被还原过一次）
    vx.update(|window, cx| {
        let _ = window.draw(cx);
        let _ = window.draw(cx);
        let after = Theme::global(cx).caret;
        assert_eq!(before, after, "渲染期改的主题 token 必须在渲染内还原");
    });
}
