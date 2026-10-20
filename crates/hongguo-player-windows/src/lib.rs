//! 红果桌面版 Windows 系统播放器：Media Foundation Media Engine（rendering
//! mode）+ 自定义 `IMFByteStream` + DirectComposition 视频位。
//!
//! 与 macOS 的 `hongguo-player-macos` 一一对应，契约在 `hongguo-player-api`：
//!
//! | 职责 | macOS | Windows |
//! | --- | --- | --- |
//! | 引擎 | `AVPlayer` + `AVPlayerItem` | MediaEngine（rendering mode） |
//! | 供数 | `AVAssetResourceLoader` | 自有 `IMFByteStream`（[`stream`]） |
//! | 画面 | `AVPlayerLayer` 插在 GPUIView 之下 | `IDCompositionVisual` 插在 GPUI 内容之下（[`view`]） |
//! | 事件 | 事件线程轮询 item 状态 | `IMFMediaEngineNotify` 回调（[`events`]） |
//!
//! 平台专属依赖只在本 crate；语法与设计见 `docs/gpui-system-player-windows.md`。

#![cfg(target_os = "windows")]

mod events;
mod mf;
mod player;
mod stream;
mod view;

pub use player::open;

/// 系统后端启动时调用一次：登记宿主窗口（视频位就挂在它下面）+
/// Media Foundation 进程级初始化。
///
/// 窗口背景透明（GPUI 清屏 alpha=0 才有洞）是壳的职责——那是 UI 框架的窗口
/// 语义，不属于播放器实现；壳在调本函数之前自行设置。
pub fn prepare_window(host: &hongguo_player_api::HostWindow) {
    view::prepare_window(host);
    mf::startup();
}
