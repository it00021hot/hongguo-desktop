//! 系统播放器门面：按编译目标分派到 `hongguo-player-macos` / `hongguo-player-windows`。
//!
//! 壳只依赖本 crate：`open` 打开一集、`prepare_window` 让窗口给画面层让位。
//! 契约类型（`PlayerLike` / `PlayerEvent` / `PresenterBounds` …）原样转出，
//! 调用方不需要同时认识 api 与实现。
//!
//! 无系统播放器实现的目标（Linux 等）编译成空壳：`open` 返回 `None`、
//! `prepare_window` 空操作（壳只发 macOS / Windows，起播按失败处理）。

pub use hongguo_player_api as api;
pub use hongguo_player_api::*;

#[cfg(target_os = "macos")]
mod imp {
    pub use hongguo_player_macos::*;
}

#[cfg(target_os = "windows")]
mod imp {
    pub use hongguo_player_windows::*;
}

/// Phase 0 AVPlayer 诊断 spike（`HONGGUO_AV_SPIKE=1`）用的底层句柄，只在
/// macOS 目标存在：壳的诊断入口拿它直接驱动 AVFoundation。
#[cfg(target_os = "macos")]
pub use hongguo_player_macos::{avf, loader};

/// 打开一集系统播放器；`None` = 播放器/画面位起不来，调用方走起播失败。
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn open(
    source: PlaySource,
    start_secs: f64,
    rate: f64,
    events: tokio::sync::mpsc::Sender<PlayerEvent>,
    on_stall: Option<StallHook>,
) -> Option<Player> {
    imp::open(source, start_secs, rate, events, on_stall)
}

/// 本目标有没有（启用的）内置软解直播放（HEVC / ByteVC1 / ByteVC2 由进程内解码器解、画面自己上屏）。
/// 壳据此决定要不要把选流切到「只收内置解码器能解的编码」。
/// macOS 恒开；Windows 的实现没有在真机上验证过，默认关，`HONGGUO_WIN_DIRECT=1` 才开。
pub fn direct_playback_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        imp::direct_playback_enabled()
    }
    #[cfg(not(target_os = "windows"))]
    {
        cfg!(target_os = "macos")
    }
}

/// 打开一集内置软解直播放；`None` = 本目标没有实现或线程起不来，调用方走起播失败。
/// 与 [`open`] 同一套契约（事件、控制面、舞台几何），区别只在画面不经过系统视频解码器。
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn open_direct(
    source: PlaySource,
    start_secs: f64,
    rate: f64,
    events: tokio::sync::mpsc::Sender<PlayerEvent>,
    on_stall: Option<StallHook>,
) -> Option<Player> {
    imp::open_direct(source, start_secs, rate, events, on_stall)
}

/// 没有直播放实现的目标：恒 `None`。壳不应在这些目标上调用它——
/// [`direct_playback_available`] 为假时选流不会切到内置模式。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn open_direct(
    _source: PlaySource,
    _start_secs: f64,
    _rate: f64,
    _events: tokio::sync::mpsc::Sender<PlayerEvent>,
    _on_stall: Option<StallHook>,
) -> Option<Player> {
    None
}

/// 系统后端启动时调用一次：把宿主窗口交给播放器实现。
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn prepare_window(host: &HostWindow) {
    imp::prepare_window(host);
}

/// 没有系统播放器实现的目标：接口保持可用，行为是「不起播」。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn open(
    _source: PlaySource,
    _start_secs: f64,
    _rate: f64,
    _events: tokio::sync::mpsc::Sender<PlayerEvent>,
    _on_stall: Option<StallHook>,
) -> Option<Player> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn prepare_window(_host: &HostWindow) {}
