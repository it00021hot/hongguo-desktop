//! 串行任务队列。
//!
//! 同一个 webview 窗口不能并发导航——后一次导航会打断前一次，
//! 导致两个嗅探任务互相污染结果。所以所有嗅探请求排队串行执行。

use tokio::sync::Mutex;

/// 串行队列。
///
/// 用 `tokio::sync::Mutex` 并在 `run` 里持有 guard 贯穿整个异步任务：
/// 这正是该锁的用法（异步互斥），不会阻塞 worker 线程。
pub struct SniffQueue {
    guard: Mutex<()>,
}

impl Default for SniffQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl SniffQueue {
    /// 构造队列。
    pub fn new() -> Self {
        Self {
            guard: Mutex::new(()),
        }
    }

    /// 排到队首执行任务；guard 释放后下一位才能进。
    pub async fn run<F, Fut, T>(&self, f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let _guard = self.guard.lock().await;
        f().await
    }
}

/// 全局队列：所有嗅探共用一个窗口，跨调用也必须串行。
static QUEUE: SniffQueue = SniffQueue {
    guard: Mutex::const_new(()),
};

/// 取全局队列引用。
pub fn global() -> &'static SniffQueue {
    &QUEUE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn tasks_run_serially() {
        // 队列要活过所有 spawn，用 Arc 而不是借用
        let q = Arc::new(SniffQueue::new());
        let concurrent = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..4 {
            let q = q.clone();
            let concurrent = concurrent.clone();
            let peak = peak.clone();
            handles.push(tokio::spawn(async move {
                q.run(|| async move {
                    let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                })
                .await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 1, "不应出现并发执行");
    }

    #[tokio::test]
    async fn global_queue_is_usable() {
        let n = global().run(|| async { 42 }).await;
        assert_eq!(n, 42);
    }
}
