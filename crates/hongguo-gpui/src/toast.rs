//! 壳内轻量提示（Tauri toast 的最小对齐面）：舞台顶部居中浮字，数秒自灭。
//!
//! 单槽后到覆盖先到——弹幕/互动反馈都是单发短消息，不需要堆叠队列。收尾
//! 靠出显时的定时任务，不依赖重渲染节拍；渲染挂在舞台层（视频之上，无
//! 指针监听不吃点击）。

use std::time::{Duration, Instant};

/// 单条提示的停留时长。
pub(crate) const TOAST_TTL: Duration = Duration::from_secs(3);

/// 一条在显的提示。
#[derive(Clone)]
pub(crate) struct Toast {
    pub(crate) message: String,
    shown_at: Instant,
}

impl Toast {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            shown_at: Instant::now(),
        }
    }

    /// 是否已到收尾时刻。
    pub(crate) fn expired(&self) -> bool {
        self.shown_at.elapsed() >= TOAST_TTL
    }
}
