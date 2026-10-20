//! 壳内播放器门面：GPUI 自研引擎与系统播放器（AVPlayer）共用同一组方法。
//!
//! 视图只认 [`PlayerLike`]——业务/界面代码不写平台分支，也不直接依赖
//! `hongguo_playback::PlayerHandle`；系统实现放在 `crate::av_presenter`
//! （macOS 专属），非 macOS 目标上 `VIDEO_BACKEND=system` 会被
//! [`crate::video_backend`] 拒绝，永远拿不到系统实现。

use std::sync::Arc;

use gpui_kit::Window;
use hongguo_playback::{PlayerHandle, PresenterBounds, PresenterFit};

/// 视图侧播放器句柄（`Arc` 便于事件回调/后台任务共享）。
pub(crate) type Player = Arc<dyn PlayerLike>;

/// 两种后端共用的控制面。方法名与 `PlayerHandle` 一一对应，视图调用点
/// 不需要改；平台差异（画面挂载/舞台矩形）走默认空实现。
pub(crate) trait PlayerLike: Send + Sync {
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
    fn attach(&self, _window: &Window) {}

    /// 舞台矩形/适配/可见性（每次 render 提交；GPUI 后端无操作）。
    fn submit_stage(&self, _bounds: Option<PresenterBounds>, _fit: PresenterFit, _visible: bool) {}

    /// 是否系统后端（视图据此打透明洞、跳过 surface 上屏）。
    fn is_system(&self) -> bool {
        false
    }

    /// 离开播放页/切剧时收起画面层（系统后端专用；GPUI 后端无操作）。
    fn detach(&self) {}

    /// 预加载下一集（系统后端 AVQueuePlayer 接力；GPUI 后端无操作）。
    fn preload_next(&self, _vid: &str, _definition: u32) -> bool {
        false
    }

    /// 切换到已预加载的下一集；返回 false = 没有预加载项。
    fn advance_preloaded(&self, _vid: &str) -> bool {
        false
    }
}

impl PlayerLike for PlayerHandle {
    fn pause(&self) {
        PlayerHandle::pause(self);
    }

    fn resume(&self) {
        PlayerHandle::resume(self);
    }

    fn is_paused(&self) -> bool {
        PlayerHandle::is_paused(self)
    }

    fn position_secs(&self) -> f64 {
        PlayerHandle::position_secs(self)
    }

    fn duration_secs(&self) -> f64 {
        PlayerHandle::duration_secs(self)
    }

    fn seek(&self, seconds: f64) {
        PlayerHandle::seek(self, seconds);
    }

    fn set_rate(&self, rate: f64) {
        PlayerHandle::set_rate(self, rate);
    }

    fn rate(&self) -> f64 {
        PlayerHandle::rate(self)
    }

    fn set_volume(&self, volume: f32) {
        PlayerHandle::set_volume(self, volume);
    }

    fn set_muted(&self, muted: bool) {
        PlayerHandle::set_muted(self, muted);
    }

    fn dims(&self) -> (usize, usize) {
        PlayerHandle::dims(self)
    }
}
