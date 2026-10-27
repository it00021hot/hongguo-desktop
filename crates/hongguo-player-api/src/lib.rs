//! 红果桌面版回放契约（不依赖任何 UI 框架与平台实现）。
//!
//! 系统播放器（`hongguo-player-macos` / `hongguo-player-windows`）实现这一
//! 层：AVPlayer / Media Foundation Media Engine，画面走原生合成层。
//!
//! 壳只认本 crate 的 [`PlayerLike`] 与 [`PlayerEvent`]，不写平台分支；
//! 打开/窗口准备由门面 crate `hongguo-player` 分派。
//!
//! 本 crate 的依赖纪律（契约与实现分离）：
//! 只允许窗口句柄（`raw-window-handle`）这类中立依赖；**不得**依赖壳或
//! 任何平台实现。

mod event;
mod host;
mod player;
mod presenter;

pub use event::{PlaySource, PlayerEvent};
pub use host::HostWindow;
pub use player::{Player, PlayerLike, StallHook};
pub use presenter::{PresenterBounds, PresenterFit, VideoPresenter};
