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
    /// 轮询途中被更新的请求取代，主动放弃
    Abandoned,
}

/// 轮询直到 `probe` 返回非空、超时，或 `abandon` 说该放弃了。
///
/// `abandon` 是换筛选时的逃生门：用户在 15 秒轮询途中又点了别的分类，
/// 这时继续等下去毫无意义 —— 结果出来也已经被新请求取代，用户只看到白等。
pub async fn poll_until<F, Fut, A>(probe: F, abandon: A) -> PollOutcome
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = String>,
    A: Fn() -> bool,
{
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if abandon() {
            return PollOutcome::Abandoned;
        }
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
        let out = poll_until(
            || async { r#"[{"series_id":"123456"}]"#.to_string() },
            || false,
        )
        .await;
        assert_eq!(
            out,
            PollOutcome::Got(r#"[{"series_id":"123456"}]"#.to_string())
        );
    }

    #[tokio::test]
    async fn empty_results_keep_polling() {
        let out = poll_until(|| async { "[]".to_string() }, || false).await;
        assert_eq!(out, PollOutcome::TimedOut, "空数组不应算命中");
    }

    #[tokio::test]
    async fn empty_object_keeps_polling() {
        let out = poll_until(|| async { "{}".to_string() }, || false).await;
        assert_eq!(out, PollOutcome::TimedOut, "空对象不应算命中");
    }

    #[test]
    fn intervals_match_legacy() {
        assert_eq!(TIMEOUT, Duration::from_secs(15));
        assert_eq!(INTERVAL, Duration::from_millis(400));
    }

    #[tokio::test]
    async fn abandons_before_timing_out() {
        // 换了筛选就该立刻放手，而不是再空转 15 秒
        let out = poll_until(|| async { "[]".to_string() }, || true).await;
        assert_eq!(out, PollOutcome::Abandoned);
        assert_ne!(out, PollOutcome::TimedOut, "放弃不算超时");
    }
}
