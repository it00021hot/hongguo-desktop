//! 播放器控制面契约：视图只认 [`PlayerLike`]，不直接依赖任何平台实现。
//!
//! 平台差异（画面挂载/舞台矩形）走默认空实现——不实现某平台能力的宿主
//! 调到空操作即可，不必写平台分支。

use std::sync::Arc;

use crate::{HostWindow, PresenterBounds, PresenterFit};

/// 视图侧播放器句柄（`Arc` 便于事件回调/后台任务共享）。
pub type Player = Arc<dyn PlayerLike>;

/// 供数读到填充前沿/超时时的业务续填钩子。
pub type StallHook = Arc<dyn Fn() + Send + Sync>;

/// 播放器控制面（系统播放器实现，macOS AVPlayer / Windows MediaEngine）。
pub trait PlayerLike: Send + Sync {
    fn pause(&self);
    fn resume(&self);
    fn is_paused(&self) -> bool;
    fn position_secs(&self) -> f64;
    fn duration_secs(&self) -> f64;
    fn seek(&self, seconds: f64);
    fn set_rate(&self, rate: f64);
    fn rate(&self) -> f64;
    fn set_volume(&self, volume: f32);
    fn set_muted(&self, muted: bool);
    fn dims(&self) -> (usize, usize);

    /// 把画面层挂到真实窗口。
    fn attach(&self, _host: &HostWindow) {}

    /// 舞台矩形/适配/可见性（每次 render 提交）。
    fn submit_stage(&self, _bounds: Option<PresenterBounds>, _fit: PresenterFit, _visible: bool) {}

    /// 离开播放页/切剧时收起画面层。
    fn detach(&self) {}

    /// 预加载下一集（队列接力）。
    fn preload_next(&self, _vid: &str, _definition: u32, _on_stall: Option<StallHook>) -> bool {
        false
    }

    /// 切换到已预加载的下一集；返回 false = 没有预加载项。
    fn advance_preloaded(&self, _vid: &str) -> bool {
        false
    }
}
