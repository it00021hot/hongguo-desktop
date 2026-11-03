//! 控制区（对齐 Tauri 版控制栏核心件）：暂停/倍速/音量/清晰度入口、
//! 进度轨道（拖动 seek 与节流）、控制条显隐、偏好落盘、引擎事件分发，
//! 以及大屏控制条渲染。

use std::time::Duration;

use gpui_kit::Window;
use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::div;
use gpui_kit::prelude::*;

use super::HomeView;

/// 控制条强制常显时长（秒）：交互后多 linger 一段时间再交回闲置淡出。
/// 只被 screenshot 门控的取证辅助使用，随消费点一起进 feature。
#[cfg(feature = "screenshot")]
pub(super) const CONTROLS_HOLD_SECS: u64 = 600;

mod bar;
mod behavior;
mod danmaku_bar;
mod menus;
mod mini;
mod progress;
mod volume;

/// 播放器浮层钮的皮肤：透明底、白字、hover 铺 20% 白——颜色不跟主题走。
/// 必须用 `custom` 变体：`ghost` 的前景/hover 取自主题，亮色主题下是深色字
/// 压在画面上；且 gpui Button 内部已占用 hover 层、实例 `text_color` 盖不住
/// 悬停态，只有 custom 变体能一次钉死三态颜色并保留 tooltip/焦点语义。
pub(super) fn chrome_variant(cx: &gpui_kit::App) -> ButtonCustomVariant {
    let transparent: gpui_kit::Hsla = gpui_kit::rgba(0x00000000).into();
    ButtonCustomVariant::new(cx)
        .color(transparent)
        .foreground(gpui_kit::rgba(0xffffffe6).into())
        .hover(gpui_kit::rgba(0xffffff33).into())
        .active(gpui_kit::rgba(0xffffff4d).into())
}

/// 播放器/评论面板里的输入框：在元素真正渲染的那一拍，临时把主题里光标/
/// 占位符/选区/编辑器底色四个 token 换成播放器皮肤值，再交组件渲染后立刻
/// 还原。只能这么做：`Input` 在 render 里从 `cx.theme()` 拷走颜色（crate
/// 内部 API），应用侧没有实例级覆盖入口；改写与还原在同一同步渲染调用内
/// 完成。编辑器底色必须显式透明——editor 元素自己会逐行涂
/// `editor_background`，不设就是亮色白底块。
#[derive(gpui_kit::IntoElement)]
pub(super) struct SkinnedInput {
    inner: gpui_kit::component::input::Input,
}

impl gpui_kit::RenderOnce for SkinnedInput {
    fn render(
        self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::component::Theme;
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
            // Arc 容器：首次经 make_mut 拆出本壳自有副本，之后就地改写。
            std::sync::Arc::make_mut(&mut theme.highlight_theme)
                .style
                .editor_background = Some(gpui_kit::rgba(0x00000000).into());
            saved
        };
        // Input::render 在这里同步读取主题并把这几个颜色拷进 InputState。
        // `into_any_element()` 把返回的元素摊成自有值：它默认会借住 `cx`
        // （RPIT 捕获入参生命周期），不摊平的话下面还原借不到可变引用。
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

/// 给播放器 chrome 的输入框套上皮肤（`Input` 的 builder 照常配）。
pub(super) fn skinned_input(input: gpui_kit::component::input::Input) -> SkinnedInput {
    SkinnedInput { inner: input }
}

/// 进度拖动的拖拽载荷（`on_drag`/`on_drag_move` 按类型配对）。
struct ScrubDrag;

/// 音量竖条拖拽载荷（与进度拖动同套 on_drag 机制）。
struct VolumeDrag;

/// 拖动期间跟指针的幽灵视图：空 div（拖动不需要可视残影，预览走进度条本身）。
struct ScrubGhost;

impl gpui_kit::Render for ScrubGhost {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        div()
    }
}

impl super::HomeView {
    /// 倍速档位（与 Tauri 版 `player-controls.tsx` 的 RATES 完全一致）。
    const RATES: [f64; 6] = [0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
    /// 控制条可见时长：鼠标静止/无操作后自动隐藏。
    pub(super) const CONTROLS_VISIBLE: Duration = Duration::from_secs(3);
    /// 拖动中实时 seek 的最小间隔：引擎一次 seek = 冲流 + 解码器重建，
    /// 250ms 限频跟手又不拖垮供给（Tauri 侧浏览器自行合并，这里显式限）。
    const SCRUB_SEEK_MIN_INTERVAL: Duration = Duration::from_millis(250);
}
