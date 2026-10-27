//! 共用任务运行时：进程级多线程 Tokio 运行时。
//!
//! 定位与 Tauri 的 async_runtime 等价（语义上是同一回事）：全局运行时，
//! **任意线程可调** [`spawn`]。service 会被协议回调、主线程等无 Tokio
//! 上下文的线程调用（见红线 1），后台任务不得依赖
//! 调用点上下文；两个壳共用本机制，不再把壳专属运行时传进 service。
//!
//! 与壳的分工：Tauri 壳的 command 自身仍跑在 Tauri 的运行时上（那是壳的
//! IPC 设施）；service 内部的长期任务（下载调度、取流填充、登录轮询、
//! 资料刷新、阻塞重活）一律由本运行时承载。

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Handle, Runtime};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// 取进程级任务运行时（首次调用时懒初始化；多线程 + 全部驱动）。
pub fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("hongguo-task")
            .build()
            .expect("hongguo-service 任务运行时初始化失败")
    })
}

/// 任意线程可调的后台任务派发（调用面与 Tauri 的 async_runtime::spawn 相同）。
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(future)
}

/// 阻塞任务派发（专用阻塞池，不占 async 工作线程）。
pub fn spawn_blocking<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    runtime().spawn_blocking(f)
}

/// 运行时 handle（少数需要显式持有运行时的设施，如跨线程回调里派发）。
pub fn handle() -> Handle {
    runtime().handle().clone()
}
