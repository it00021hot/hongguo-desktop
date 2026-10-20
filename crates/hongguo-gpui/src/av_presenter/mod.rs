//! 系统播放器（macOS AVPlayer）呈现实现。
//!
//! `VIDEO_BACKEND=system` 时由 GPUI 壳的 `home::player` 门面选用；默认 gpui
//! 后端完全不加载本模块的代码路径。平台专属依赖只在这里出现。

pub(crate) mod avf;
pub(crate) mod loader;
mod player;
pub(crate) mod view;

pub(crate) use player::open;

/// 系统后端启动时调用一次：窗口透明 + 背景铺黑，给视频层让出洞。
pub(crate) fn prepare_window(window: &gpui_kit::Window) {
    view::prepare_window(window);
}
