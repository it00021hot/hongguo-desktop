//! 协议路径的常驻工作池与 panic 加固：协议回调跑在主线程 extern "C" 边界里，panic 无法 unwind 出去，std 只能 abort 全进程。
//! 请求不由「一条一线程」消费，而由常驻工作池承接；回调体与 serve 的 `catch_unwind` 防线在壳层注册处。

use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

/// 一条后台任务。池派发的单位；壳层注册处把「取参 + 供数 + 回包」打包进来。
pub type Job = Box<dyn FnOnce() + Send + 'static>;

/// 流池大小：在线播放的 Range 请求（serve 内部有最长 30s 的等数据循环），
/// 与浏览器对单 host 的并发上限（≤6）对齐。
const STREAM_WORKERS: usize = 6;
/// 文件池大小：本地文件读 + 封面下载转码。与流池分开，流的长时间等待
/// 不至于把封面/本地播放饿死。
const FILE_WORKERS: usize = 6;

static STREAM_POOL: OnceLock<WorkerPool> = OnceLock::new();
static FILE_POOL: OnceLock<WorkerPool> = OnceLock::new();

/// 常驻工作线程池。进程生命周期内不销毁——退出时 worker 阻塞在 recv 上，
/// 随进程一起消亡，不存在「退出中还要起线程」的窗口。
pub struct WorkerPool {
    tx: Sender<Job>,
}

impl WorkerPool {
    fn new(workers: usize, prefix: &str) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = std::sync::Arc::new(Mutex::new(rx));
        let prefix = prefix.to_string();
        for i in 0..workers {
            // Builder::spawn 返回 Result 而非 panic：个别 worker 起不来只是
            // 池变小，不构成事件
            let _ = std::thread::Builder::new()
                .name(format!("{prefix}-{i}"))
                .spawn({
                    let rx = std::sync::Arc::clone(&rx);
                    let prefix = prefix.clone();
                    move || loop {
                        // 锁只在 recv 期间持有，任务在锁外执行，panic 毒不了这把锁
                        let job = rx
                            .lock()
                            .expect("池接收端的锁只在 recv 时持有，不会中毒")
                            .recv();
                        match job {
                            Ok(job) => {
                                if let Err(p) = catch_unwind(AssertUnwindSafe(job)) {
                                    log::error!(
                                        "[Protocol] {prefix} 工作线程任务 panic（已拦截）: {}",
                                        panic_message(&p)
                                    );
                                }
                            }
                            Err(_) => break, // 发送端是 static，正常到不了这里
                        }
                    }
                });
        }
        Self { tx }
    }

    /// 派发一条任务。池还在就进队列；池全灭（理论下限）回退直启线程，再
    /// 失败就记日志放弃——这条链路上没有任何会 panic 的调用。
    fn dispatch(&self, job: Job) {
        if let Err(err) = self.tx.send(job) {
            log::error!("[Protocol] 工作池不可用，回退直启线程");
            let _ = std::thread::Builder::new()
                .name("hongguo-proto-fallback".into())
                .spawn(move || (err.0)());
        }
    }
}

/// 在线流请求的工作池（进程级单例）。
pub fn stream_pool() -> &'static WorkerPool {
    STREAM_POOL.get_or_init(|| WorkerPool::new(STREAM_WORKERS, "hongguo-proto-stream"))
}

/// 本地文件与封面请求的工作池（进程级单例）。
pub fn file_pool() -> &'static WorkerPool {
    FILE_POOL.get_or_init(|| WorkerPool::new(FILE_WORKERS, "hongguo-proto-file"))
}

/// 回调统一外壳：解析与派发都包在 `catch_unwind` 里——panic 与 FFI 边界之间的最后一道闸，新 panic 源只损失一条请求，进程不 abort。
pub fn guarded(pool: &'static WorkerPool, scheme: &str, mk_job: impl FnOnce() -> Job) {
    match catch_unwind(AssertUnwindSafe(mk_job)) {
        Ok(job) => pool.dispatch(job),
        Err(p) => log::error!(
            "[Protocol] {scheme} 回调本体 panic（已拦截，未穿透 FFI）: {}",
            panic_message(&p)
        ),
    }
}

/// 把 `catch_unwind` 抓到的 payload 转成可读文本（默认 panic hook 只写
/// stderr，这里同时落到日志，方便事后从日志定位）。
pub fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "（非文本 panic payload）".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn pool_serves_jobs_after_a_panicking_one() {
        let (tx, rx) = mpsc::channel();
        super::file_pool().dispatch(Box::new(move || panic!("模拟封面转码 panic")));
        super::file_pool().dispatch(Box::new(move || tx.send(1).unwrap()));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            1,
            "panic 的任务不能拖垮工作池"
        );
    }
}
