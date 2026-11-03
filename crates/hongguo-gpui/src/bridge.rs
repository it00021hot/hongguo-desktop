//! tokio 桥：把 service 的 async 调用投到常驻 tokio 运行时，经 oneshot
//! 回传给 GPUI 执行器。不变量：`spawn` 总是把发送端移进运行时任务、任务必送
//! 回 Ok/Err，因此壳里 `rx.await.expect("tokio 桥不会丢弃发送端")` 是不变量
//! 断言而非吞错——新增调用沿用同一措辞，别在此处加兜底。

use std::future::Future;

use tokio::sync::oneshot;

/// 把一个 future 丢进常驻 tokio 运行时执行，返回可跨执行器 await 的接收端。
pub(crate) fn spawn<T, F>(future: F) -> oneshot::Receiver<T>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    crate::tokio().spawn(async move {
        let _ = tx.send(future.await);
    });
    rx
}
