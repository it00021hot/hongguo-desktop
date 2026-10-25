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

/// 播放器浮层钮的皮肤（Tauri `IconButton` 同款）：透明底、白字、
/// hover 铺 20% 白——**颜色不跟主题走**。
///
/// 为什么非要用 `custom` 变体：`ghost` 的前景与 hover 文字色都取自主题
/// （`secondary_foreground`），亮色主题下就是深色字压在画面上（用户实测
/// 「亮色主题下都看不到按钮」）；而 gpui 的 `Button` 内部已占用 hover 层，
/// 实例 `text_color` 盖不住悬停态，只有 custom 变体能同时把 normal/hover/active
/// 三态颜色一次钉死，同时保留 Button 的 tooltip / 焦点语义。
pub(super) fn chrome_variant(cx: &gpui_kit::App) -> ButtonCustomVariant {
    let transparent: gpui_kit::Hsla = gpui_kit::rgba(0x00000000).into();
    ButtonCustomVariant::new(cx)
        .color(transparent)
        .foreground(gpui_kit::rgba(0xffffffe6).into())
        .hover(gpui_kit::rgba(0xffffff33).into())
        .active(gpui_kit::rgba(0xffffff4d).into())
}

/// 播放器/评论面板里的输入框：在元素**真正渲染的那一拍**，临时把主题里
/// 光标 / 占位符 / 选区 / **编辑器底色**四个 token 换成播放器皮肤值，
/// 再把 `Input` 交给组件渲染，然后立刻还原。
///
/// 为什么只能这么做：`Input` 在 `RenderOnce::render` 里从 `cx.theme()` 把
/// 这几个颜色拷进 `InputState` 的 editor style（`set_editor_style` 是
/// gpui-component 的 crate 内部 API），应用侧没有实例级覆盖入口。播放器是
/// 固定深色皮肤，亮色主题的默认值是「近黑光标 + 深灰占位符」，压在深色
/// 胶囊/面板上就是隐形。`Theme::global_mut` 不广播主题变更、不动其它字段
/// （见其注释），我们的改写与还原在同一个同步渲染调用内完成，正文色仍由
/// 调用方的 `.text_color()` 决定（正文取继承色）。
///
/// 编辑器底色必须显式透明：`appearance(false)` 只关掉 `Input` 容器的底色，
/// 而 gpui-base 的 editor 元素自己会**逐行涂 `editor_background`**（未设置
/// 时回落 `input_background()`——亮色下就是白底块，实测两种主题都穿了帮，
/// 见 feed-comments 预览）。底色由外层胶囊/输入框容器画。
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
