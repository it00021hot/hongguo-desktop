//! 红果桌面版回放契约（不依赖任何 UI 框架与平台实现）。
//!
//! 两个壳、两种后端共用这一层：
//!
//! - **自研引擎**（`hongguo-playback`）：解复用 + 平台解码 + 音频时钟；
//! - **系统播放器**（`hongguo-player-macos` / `hongguo-player-windows`）：
//!   AVPlayer / Media Foundation Media Engine，画面走原生合成层。
//!
//! 壳只认本 crate 的 [`PlayerLike`] 与 [`PlayerEvent`]，不写平台分支；
//! 系统播放器的打开/窗口准备由门面 crate `hongguo-player` 分派。
//!
//! 本 crate 的依赖纪律（`docs/architecture-refactor.md` §4「契约与实现分离」）：
//! 只允许帧载体（`hongguo-media-platform-api`）与窗口句柄
//! （`raw-window-handle`）这类中立依赖；**不得**依赖引擎、壳或任何平台实现。

mod event;
mod host;
mod player;
mod presenter;

pub use event::{OpenOptions, PlaySource, PlayerEvent};
pub use host::HostWindow;
pub use player::{Player, PlayerLike, StallHook};
pub use presenter::{PresenterBounds, PresenterFit, VideoPresenter};
