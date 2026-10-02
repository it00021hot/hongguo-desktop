//! 嗅探轮询。
//!
//! 页面是 SPA，加载完才有内容，所以要反复尝试直到命中或超时。
//! 15 秒超时、每 400 毫秒重试——与现版一致。

use std::time::{Duration, Instant};

/// 总超时。
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// 重试间隔。
pub const INTERVAL: Duration = Duration::from_millis(400);

/// 轮询结果。
#[derive(Debug, Clone, PartialEq)]
pub enum PollOutcome {
    /// 已拿到结果
    Got(String),
    /// 超时仍未拿到
    TimedOut,
}

/// 轮询直到 `probe` 返回非空或超时。
pub async fn poll_until<F, Fut>(probe: F) -> PollOutcome
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = String>,
{
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let raw = probe().await;
        if !raw.is_empty() && raw != "[]" && raw != "{}" {
            return PollOutcome::Got(raw);
        }
        if Instant::now() >= deadline {
            return PollOutcome::TimedOut;
        }
        tokio::time::sleep(INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_immediately_on_hit() {
        // probe 返回非空 JSON，第一次就应命中
        let out = poll_until(|| async { r#"[{"series_id":"123456"}]"#.to_string() }).await;
        assert_eq!(
            out,
            PollOutcome::Got(r#"[{"series_id":"123456"}]"#.to_string())
        );
    }

    #[tokio::test]
    async fn empty_results_keep_polling() {
        let out = poll_until(|| async { "[]".to_string() }).await;
        assert_eq!(out, PollOutcome::TimedOut, "空数组不应算命中");
    }

    #[tokio::test]
    async fn empty_object_keeps_polling() {
        let out = poll_until(|| async { "{}".to_string() }).await;
        assert_eq!(out, PollOutcome::TimedOut, "空对象不应算命中");
    }

    #[test]
    fn intervals_match_legacy() {
        assert_eq!(TIMEOUT, Duration::from_secs(15));
        assert_eq!(INTERVAL, Duration::from_millis(400));
    }
}
