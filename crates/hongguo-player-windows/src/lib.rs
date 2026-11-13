//! 红果桌面版 Windows 系统播放器：Media Foundation Media Engine（frame-server 模式）+ 自定义 `IMFByteStream` + 视频 quad（与 GPUI 内容同画布）。
//! 与 macOS 的 `hongguo-player-macos` 一一对应，契约在 `hongguo-player-api`：引擎 MediaEngine / 供数自有 `IMFByteStream`（`stream`）/
//! 画面每帧 `TransferVideoFrame` → 视频 quad 渲染进同一渲染目标（`view`）/ 事件 `IMFMediaEngineNotify` 回调（`events`），对位 macOS 的 AVPlayer / AVAssetResourceLoader / AVPlayerLayer 插在 GPUIView 之下 / 事件线程轮询。
//! 平台专属依赖只在本 crate；语法与设计见 `docs/gpui-system-player-windows.md`。
//! 另有内置软解直播放（`direct`：引擎只放「只剩音轨」视图并充当时钟，`hongguo-softdec` 解出的帧换成 BGRA 写进同一个视频 quad），
//! HEVC / ByteVC1 / ByteVC2 默认使用内置解码，原生视频路径供本地 H.264 使用。

#![cfg(target_os = "windows")]

mod direct;
mod events;
mod mf;
mod player;
mod stream;
mod view;

pub use direct::open as open_direct;
pub use player::open;

/// 系统后端启动时调用一次：登记宿主窗口（视频 quad 挂在它上面）+ Media Foundation 进程级初始化。
/// 窗口背景透明（GPUI 清屏 alpha=0 才有洞）是壳的职责——那是 UI 框架的窗口语义，不属于播放器实现，壳在调本函数之前自行设置。
pub fn prepare_window(host: &hongguo_player_api::HostWindow) {
    view::prepare_window(host);
    mf::startup();
}
