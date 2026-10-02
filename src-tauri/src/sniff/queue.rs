//! 串行任务队列。
//!
//! 同一个 webview 窗口不能并发导航——后一次导航会打断前一次，
//! 导致两个嗅探任务互相污染结果。所以所有嗅探请求排队串行执行。

use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::Mutex;

/// 串行队列。
///
/// 用 `tokio::sync::Mutex` 并在 `run` 里持有 guard 贯穿整个异步任务：
/// 这正是该锁的用法（异步互斥），不会阻塞 worker 线程。
///
/// 排队时不做任何事有个明显的坑：用户连点三个分类，就会排出三个嗅探任务，
/// 而每个最坏要等满 15 秒超时。真正要看的是第三个，前两个的结果出来也已经被
/// 前端丢弃了。所以这里记一个代号，**轮到谁时发现已经有更新的请求，就直接放弃**。
pub struct SniffQueue {
    guard: Mutex<()>,
    /// 每接一个任务 +1；任务开始执行时读一次，跟它自己的代号比。
    latest: AtomicU64,
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
            latest: AtomicU64::new(0),
        }
    }

    /// 登记一个新任务，返回它的代号。
    fn issue_token(&self) -> u64 {
        self.latest.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// 这个任务是否已经过期（后面又来了更新的请求）。
    ///
    /// 排队中的任务靠它在开头判断；**正在跑**的任务要靠它中途退出 ——
    /// 换分类时，前一个嗅探可能正卡在 15 秒轮询里，不中断的话用户要白等到超时。
    pub fn superseded(&self, token: u64) -> bool {
        self.latest.load(Ordering::SeqCst) != token
    }

    /// 排到队首执行任务；guard 释放后下一位才能进。
    ///
    /// 闭包拿到自己的代号，配合 [`Self::superseded`] 在执行途中判断是否该放弃。
    /// 返回 `None` 表示「轮到我了，但已经有更新的请求在排队，不必做」。
    pub async fn run<F, Fut, T>(&self, f: F) -> Option<T>
    where
        F: FnOnce(u64) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let token = self.issue_token();
        let _guard = self.guard.lock().await;
        if self.superseded(token) {
            log::debug!("[Sniff] 任务 #{token} 已被更新的请求取代，跳过");
            return None;
        }
        Some(f(token).await)
    }
}

/// 全局队列：所有嗅探共用一个窗口，跨调用也必须串行。
static QUEUE: SniffQueue = SniffQueue {
    guard: Mutex::const_new(()),
    latest: AtomicU64::new(0),
};

/// 取全局队列引用。
pub fn global() -> &'static SniffQueue {
    &QUEUE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
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
                q.run(|_token| async move {
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
        let n = global().run(|_token| async { 42 }).await;
        assert_eq!(n, Some(42));
    }

    #[tokio::test]
    async fn superseded_queued_tasks_are_skipped() {
        // 模拟「连点三个分类」：第一个已经在跑（拦不住），中间那个在排队。
        // 真正要看的是中间那个轮到时会不会发现代号已旧——它要是也跑一遍，
        // 用户就得连等三轮，每次最坏 15 秒。
        let q = Arc::new(SniffQueue::new());
        let ran = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..3 {
            let q = q.clone();
            let ran = ran.clone();
            handles.push(tokio::spawn(async move {
                q.run(|_token| async move {
                    ran.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    "done"
                })
                .await
            }));
            // 错开提交，确保三者都排在同一个执行里
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(
            ran.load(Ordering::SeqCst),
            2,
            "第一个已在执行拦不住，最后一个才是用户要的，中间那个必须跳过"
        );
    }

    #[tokio::test]
    async fn an_in_flight_task_can_bail_out_midway() {
        // 换分类时，前一个嗅探正卡在 15 秒轮询里。令牌一变就该退出，
        // 否则用户点完筛选要白等到超时才看到反应。
        let q = Arc::new(SniffQueue::new());

        let handle = tokio::spawn({
            let q = q.clone();
            let check = q.clone();
            async move {
                q.run(move |token| {
                    let check = check.clone();
                    async move {
                        for _ in 0..50 {
                            if check.superseded(token) {
                                return "aborted";
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                        }
                        "completed"
                    }
                })
                .await
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        // 新筛选进来会领一个新代号，在途任务下一轮就该发现自己过期了
        q.issue_token();

        assert_eq!(
            handle.await.unwrap(),
            Some("aborted"),
            "令牌变化后应立刻放弃，而不是跑满 50 轮"
        );
    }

    #[tokio::test]
    async fn latest_token_advances_per_task() {
        let q = SniffQueue::new();
        let a = q.issue_token();
        let b = q.issue_token();
        assert!(b > a);
        assert!(q.superseded(a));
        assert!(!q.superseded(b));
    }
}
