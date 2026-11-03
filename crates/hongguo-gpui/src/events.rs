//! 服务事件泵：订阅 `hongguo_service::events` 的 tokio broadcast，转发进
//! mpsc 无界通道——接收端不依赖 tokio 上下文，可在 GPUI 执行器上直接 await
//! （与 `bridge.rs` 同款约定）；消费方构造时起 spawn 循环落到视图状态。

use hongguo_service::app_state::AppState;
use hongguo_service::events::ServiceEvent;
use tokio::sync::mpsc;

/// 订阅服务事件，返回 GPUI 侧可 await 的接收端。
pub(crate) fn subscribe(state: &AppState) -> mpsc::UnboundedReceiver<ServiceEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut events = state.events().subscribe();
    hongguo_service::runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(ev) => {
                    if tx.send(ev).is_err() {
                        break; // 消费方已退出
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("[Events] GPUI 事件泵滞后，丢弃 {n} 条旧事件");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    rx
}
