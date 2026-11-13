//! 内置视频播放门面：按编译目标分派到 macOS / Windows 实现。
//!
//! 壳只依赖本 crate：`open_direct` 打开一集，`prepare_window` 准备宿主画面位。
//! 契约类型（`PlayerLike` / `PlayerEvent` / `PresenterBounds` …）原样转出，
//! 调用方不需要同时认识 api 与实现。
//!
//! 无内置播放实现的目标（Linux 等）编译成空壳：`open_direct` 返回 `None`、
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

/// 打开一集内置软解直播放；`None` = 本目标没有实现或线程起不来，调用方走起播失败。
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

/// 没有内置视频解码实现的目标：恒 `None`，调用方按起播失败处理。
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn prepare_window(_host: &HostWindow) {}
