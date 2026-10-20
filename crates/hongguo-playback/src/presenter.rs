//! 系统播放器呈现层契约（壳无关）。
//!
//! [`VideoPresenter`] 只描述「画面往哪儿摆、怎么适配、可见性」这些宿主能
//! 控制的量；播放控制语义（play/pause/seek/rate）与具体平台实现绑定。
//! macOS 的 `AVPlayer` + `AVPlayerLayer` 实现放在 GPUI 壳的 `av_presenter`
//! 模块（平台专属依赖只在那里出现）；本 crate 不依赖任何 UI 框架。

/// 舞台矩形（逻辑像素，窗口内容区左上原点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresenterBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PresenterBounds {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width < 1.0 || self.height < 1.0
    }
}

/// 画面适配：contain 留黑边，cover 铺满裁切（小屏形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenterFit {
    Contain,
    Cover,
}

/// 系统播放器的呈现面。
///
/// 实现必须满足：`submit` / `set_visible` 只在主线程调用；`play` / `pause` /
/// `seek` / `set_rate` 可跨线程调用（由 [`PlayerLike`](crate) 门面转发）。
pub trait VideoPresenter: Send + Sync {
    /// 提交舞台矩形与适配模式。`bounds = None` 表示还没有布局结果（先保持
    /// 上一次的几何，避免切形态瞬间缩成 0×0）。
    fn submit(&self, bounds: Option<PresenterBounds>, fit: PresenterFit);

    /// 显示/隐藏画面层（离开首页、切到非播放页、老板键隐藏窗口）。
    fn set_visible(&self, visible: bool);

    fn play(&self);
    fn pause(&self);
    fn seek(&self, seconds: f64);
    fn set_rate(&self, rate: f64);

    /// 丢弃当前画面（切剧/失败态）。不等于停播，语义由实现决定。
    fn flush(&self);
}
