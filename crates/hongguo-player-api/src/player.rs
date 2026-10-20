//! 播放器门面接口：两个后端（自研引擎 / 系统播放器）共用同一组控制方法。
//!
//! 视图只认 [`PlayerLike`]——业务与界面代码不写平台分支，也不直接依赖
//! `hongguo_playback::PlayerHandle` 或任何平台实现。

use std::sync::Arc;

use crate::{HostWindow, PresenterBounds, PresenterFit};

/// 视图侧播放器句柄（`Arc` 便于事件回调/后台任务共享）。
pub type Player = Arc<dyn PlayerLike>;

/// 供数读到填充前沿/超时时的业务续填钩子（系统后端专用）。
pub type StallHook = Arc<dyn Fn() + Send + Sync>;

/// 两种后端共用的控制面。方法名与自研引擎的 `PlayerHandle` 一一对应，视图
/// 调用点不需要改；平台差异（画面挂载/舞台矩形）走默认空实现。
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

    /// 系统播放器把画面层挂到真实窗口；GPUI 后端无操作（画面走 surface）。
    fn attach(&self, _host: &HostWindow) {}

    /// 舞台矩形/适配/可见性（每次 render 提交；GPUI 后端无操作）。
    fn submit_stage(&self, _bounds: Option<PresenterBounds>, _fit: PresenterFit, _visible: bool) {}

    /// 是否系统后端（视图据此打透明洞、跳过 surface 上屏）。
    fn is_system(&self) -> bool {
        false
    }

    /// 离开播放页/切剧时收起画面层（系统后端专用；GPUI 后端无操作）。
    fn detach(&self) {}

    /// 预加载下一集（系统后端队列接力；GPUI 后端无操作）。
    fn preload_next(&self, _vid: &str, _definition: u32, _on_stall: Option<StallHook>) -> bool {
        false
    }

    /// 切换到已预加载的下一集；返回 false = 没有预加载项。
    fn advance_preloaded(&self, _vid: &str) -> bool {
        false
    }
}
