//! 壳内播放器门面：只做两件事——转出契约类型，把自研引擎接上同一条 trait。
//!
//! 契约（`PlayerLike` / `PlayerEvent` / `PresenterBounds` / `StallHook`）在
//! `hongguo-player-api`，系统播放器实现分居 `hongguo-player-macos` /
//! `hongguo-player-windows`，由门面 crate `hongguo-player` 按目标分派。
//! 业务/界面代码只认 [`PlayerLike`]，不写平台分支。
//!
//! 为什么还需要本文件：`PlayerLike` 是外部 trait，孤儿规则不允许壳直接给
//! 外部类型（`hongguo_playback::PlayerHandle`）实现它，所以用零开销 newtype
//! 承接——仍然是「一份业务、两种后端」，只是多一层纯转发。

use std::sync::Arc;

pub(crate) use hongguo_player::{Player, PlayerLike, StallHook};

/// 自研引擎句柄 → 契约 trait 的适配器（所有方法都是纯转发）。
pub(crate) struct EnginePlayer(pub(crate) Arc<hongguo_playback::PlayerHandle>);

impl PlayerLike for EnginePlayer {
    fn pause(&self) {
        self.0.pause();
    }

    fn resume(&self) {
        self.0.resume();
    }

    fn is_paused(&self) -> bool {
        self.0.is_paused()
    }

    fn position_secs(&self) -> f64 {
        self.0.position_secs()
    }

    fn duration_secs(&self) -> f64 {
        self.0.duration_secs()
    }

    fn seek(&self, seconds: f64) {
        self.0.seek(seconds);
    }

    fn set_rate(&self, rate: f64) {
        self.0.set_rate(rate);
    }

    fn rate(&self) -> f64 {
        self.0.rate()
    }

    fn set_volume(&self, volume: f32) {
        self.0.set_volume(volume);
    }

    fn set_muted(&self, muted: bool) {
        self.0.set_muted(muted);
    }

    fn dims(&self) -> (usize, usize) {
        self.0.dims()
    }
}
