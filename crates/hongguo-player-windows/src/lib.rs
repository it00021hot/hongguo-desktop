//! Windows 内置视频播放：软解 BGRA 经视频 quad 上屏，MediaEngine 仅承载音轨与时钟。

#![cfg(target_os = "windows")]

mod direct;
mod events;
mod mf;
mod player;
mod stream;
mod view;

pub use direct::open as open_direct;

/// 系统后端启动时调用一次：登记宿主窗口（视频 quad 挂在它上面）+ Media Foundation 进程级初始化。
/// 窗口背景透明（GPUI 清屏 alpha=0 才有洞）是壳的职责——那是 UI 框架的窗口语义，不属于播放器实现，壳在调本函数之前自行设置。
pub fn prepare_window(host: &hongguo_player_api::HostWindow) {
    view::prepare_window(host);
    mf::startup();
}
