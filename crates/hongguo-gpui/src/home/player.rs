//! 壳内播放器门面：契约类型（`PlayerEvent` / `PresenterBounds` / `StallHook`）
//! 与按目标分派的 `Player` 的统一转出口，壳内代码统一从这里取，不直接摸依赖。

pub(crate) use hongguo_player::{Player, PlayerEvent, PresenterBounds, PresenterFit, StallHook};
