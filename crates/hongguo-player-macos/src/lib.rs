//! 红果桌面版 macOS 系统播放器：AVPlayer + AVPlayerLayer + AVAssetResourceLoader。
//!
//! 与 Windows 的 `hongguo-player-windows` 一一对应，契约在 `hongguo-player-api`。
//! 平台专属依赖（objc2 / AppKit / AVFoundation）只在本 crate；设计见
//! `docs/gpui-video-system-backend.md` 与 `docs/gpui-system-player-status.md`。

#![cfg(target_os = "macos")]

pub mod avf;
pub mod loader;
mod player;
pub(crate) mod view;

pub use player::open;

/// 系统后端启动时调用一次：窗口背景铺黑，给视频层让出洞。
///
/// 透明本身由壳设（GPUI 的 `set_background_appearance`）——窗口外观是 UI 框架
/// 语义，不属于播放器实现。
pub fn prepare_window(host: &hongguo_player_api::HostWindow) {
    view::prepare_window(host);
}
