//! 壳内播放器门面：契约类型的统一转出口。
//!
//! 契约（`PlayerLike` / `PlayerEvent` / `PresenterBounds` / `StallHook`）在
//! `hongguo-player-api`，系统播放器实现分居 `hongguo-player-macos` /
//! `hongguo-player-windows`，由门面 crate `hongguo-player` 按目标分派并原样
//! 转出。壳内代码统一从本模块取这些类型，不直接摸依赖。

pub(crate) use hongguo_player::{Player, PlayerEvent, PresenterBounds, PresenterFit, StallHook};
