//! 系统播放器（Windows Media Foundation Media Engine）呈现实现。
//!
//! `VIDEO_BACKEND=system` 时由 GPUI 壳的 `home::player` 门面选用；默认 gpui
//! 后端完全不加载本模块的代码路径。设计文档见
//! `docs/gpui-system-player-windows.md`，与 macOS 的 `av_presenter` 一一对应：
//!
//! | 职责 | macOS | Windows |
//! | --- | --- | --- |
//! | 引擎 | `AVPlayer` + `AVPlayerItem` | MediaEngine（rendering mode） |
//! | 供数 | `AVAssetResourceLoader` | 自有 `IMFByteStream`（[`stream`]） |
//! | 画面 | `AVPlayerLayer` 插在 GPUIView 之下 | `IDCompositionVisual` 插在 GPUI 内容之下（[`view`]） |
//! | 事件 | 事件线程轮询 item 状态 | `IMFMediaEngineNotify` 回调（[`events`]） |
//!
//! 平台专属依赖只在这里出现；`platform` 门面是唯一调用方。

mod events;
mod mf;
mod player;
mod stream;
mod view;

pub(crate) use player::open;

/// 系统后端启动时调用一次：窗口背景设成透明（清屏 alpha=0 才有洞）+
/// Media Foundation 进程级初始化。
pub(crate) fn prepare_window(window: &gpui_kit::Window) {
    view::prepare_window(window);
    mf::startup();
}
