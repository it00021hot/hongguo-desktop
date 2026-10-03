//! 进度事件发射。
//!
//! 高频进度会拖慢 UI，所以按「变化 <0.5% 或间隔 <500ms 不发」节流。
//! 节流状态按任务 id 保存，不跨任务共享。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// 节流间隔。
const MIN_INTERVAL: Duration = Duration::from_millis(500);
/// 最小百分比变化量。
const MIN_PERCENT_DELTA: f64 = 0.5;

/// 单个任务的节流状态。
#[derive(Debug, Clone, Copy)]
struct Throttle {
    last_sent_at: Instant,
    last_percent: f64,
}

/// 进度节流器。
#[derive(Default)]
pub struct ProgressThrottle {
    states: Mutex<HashMap<String, Throttle>>,
}

impl ProgressThrottle {
    /// 判断这次进度是否该发。
    pub fn should_send(&self, id: &str, percent: f64) -> bool {
        let mut guard = self.states.lock();
        match guard.get(id) {
            Some(prev) => {
                let time_ok = prev.last_sent_at.elapsed() >= MIN_INTERVAL;
                let delta_ok = (percent - prev.last_percent).abs() >= MIN_PERCENT_DELTA;
                if time_ok && delta_ok {
                    guard.insert(
                        id.to_string(),
                        Throttle {
                            last_sent_at: Instant::now(),
                            last_percent: percent,
                        },
                    );
                    true
                } else {
                    false
                }
            }
            None => {
                guard.insert(
                    id.to_string(),
                    Throttle {
                        last_sent_at: Instant::now(),
                        last_percent: percent,
                    },
                );
                true
            }
        }
    }

    /// 任务结束时清掉状态，避免 HashMap 无限增长。
    pub fn forget(&self, id: &str) {
        self.states.lock().remove(id);
    }
}

/// 事件名常量（前端 `lib/ipc/events.ts` 有一一对应的类型）。
pub mod names {
    pub const DOWNLOAD_PROGRESS: &str = "download-progress";
    pub const DOWNLOAD_TASK_ADDED: &str = "download-task-added";
    pub const DOWNLOAD_COMPLETED: &str = "download-completed";
    pub const DOWNLOAD_FAILED: &str = "download-failed";
    pub const DOWNLOAD_STOPPED: &str = "download-stopped";
    pub const DOWNLOAD_QUEUE_CHANGED: &str = "download-queue-changed";
    pub const MERGE_PROGRESS: &str = "merge-progress";
    pub const MERGE_TASK_ADDED: &str = "merge-task-added";
    pub const MERGE_COMPLETED: &str = "merge-completed";
    pub const MERGE_FAILED: &str = "merge-failed";
    /// 在线播放的取流/解密进度
    pub const ONLINE_PROGRESS: &str = "online-play-progress";
    /// 播放兼容兜底的转码进度
    pub const COMPAT_PROGRESS: &str = "compat-play-progress";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_progress_always_sends() {
        let t = ProgressThrottle::default();
        assert!(t.should_send("a", 0.0));
    }

    #[test]
    fn tiny_change_is_throttled() {
        let t = ProgressThrottle::default();
        t.should_send("a", 10.0);
        // 间隔不足 500ms 且变化 <0.5%，应被节流
        assert!(!t.should_send("a", 10.2));
    }

    #[test]
    fn large_change_still_respects_interval() {
        let t = ProgressThrottle::default();
        t.should_send("a", 10.0);
        // 变化很大，但间隔不足 500ms —— 两个条件是 AND，仍应被节流
        assert!(!t.should_send("a", 90.0));
    }

    #[tokio::test]
    async fn large_change_sends_after_interval() {
        let t = ProgressThrottle::default();
        t.should_send("a", 10.0);
        tokio::time::sleep(MIN_INTERVAL + Duration::from_millis(20)).await;
        assert!(t.should_send("a", 90.0), "间隔足够后应放行");
    }

    #[test]
    fn throttle_is_per_task() {
        let t = ProgressThrottle::default();
        assert!(t.should_send("a", 0.0));
        assert!(t.should_send("b", 0.0), "不同任务不共享节流状态");
    }

    #[test]
    fn forget_clears_state() {
        let t = ProgressThrottle::default();
        t.should_send("a", 0.0);
        t.forget("a");
        assert!(t.should_send("a", 0.0), "清除后应重新发送");
    }

    #[test]
    fn event_names_are_stable() {
        assert_eq!(names::DOWNLOAD_PROGRESS, "download-progress");
        assert_eq!(names::MERGE_PROGRESS, "merge-progress");
    }
}
