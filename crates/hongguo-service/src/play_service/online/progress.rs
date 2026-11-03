//! 在线播放进度设施：事件出口类型、节流上报器与强类型事件载荷。
//! service 只产出事件，送达方式由调用方决定；没有壳订阅时发布是静默空操作。

use std::sync::Arc;

/// 进度事件出口：service 只负责产出事件，送达方式由调用方决定
/// （生产 = 事件总线；测试可注入自己的闭包）。
pub type ProgressSink = Arc<dyn Fn(&OnlineProgressEvent) + Send + Sync>;

/// 事件总线出口：进度事件 → [`crate::events::ServiceEvent::OnlineProgress`]。
pub(crate) fn event_sink(events: &crate::events::ServiceEvents) -> ProgressSink {
    let events = events.clone();
    Arc::new(move |ev| {
        events.publish(crate::events::ServiceEvent::OnlineProgress(ev.clone()));
    })
}

/// 在线播放进度上报器：复用下载服务那套节流（0.5% 或 500ms 才发一次），
/// 取流是每秒好几 MB 的量，每次都发会把 UI 线程打满。
pub struct ProgressReporter {
    sink: ProgressSink,
    /// 前端按 `{seriesId}:{vidIndex}` 过滤事件（与 compat-play-progress 同一格式），别改回裸 vid。
    key: String,
    throttle: crate::download_service::events::ProgressThrottle,
}

impl ProgressReporter {
    pub fn new(sink: ProgressSink, key: String) -> Self {
        Self {
            sink,
            key,
            throttle: Default::default(),
        }
    }

    /// 报一次进度。`total` 为 0（CDN 没给 Content-Length）时不发百分比。
    pub fn report(&self, received: u64, total: u64, phase: &str) {
        let percent = if total > 0 {
            received as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        if !self.throttle.should_send(&self.key, percent) {
            return;
        }
        (self.sink)(&OnlineProgressEvent {
            key: self.key.clone(),
            received,
            total,
            percent: percent.min(100.0),
            phase: phase.to_string(),
        });
    }

    /// 100%：解密完成、缓冲就绪。
    pub fn done(&self) {
        self.throttle.forget(&self.key);
        (self.sink)(&OnlineProgressEvent {
            key: self.key.clone(),
            received: 0,
            total: 0,
            percent: 100.0,
            phase: "ready".to_string(),
        });
    }
}

/// 在线播放进度事件（`online-play-progress`，强类型载荷）。
/// `key` 是 `{seriesId}:{vidIndex}`，前端按它过滤事件——别改回裸 vid。
#[derive(Debug, Clone, serde::Serialize)]
pub struct OnlineProgressEvent {
    pub key: String,
    pub received: u64,
    pub total: u64,
    pub percent: f64,
    pub phase: String,
}
