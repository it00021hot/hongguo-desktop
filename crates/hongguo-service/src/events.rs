//! 业务事件总线：service 发布，壳订阅适配（去 Tauri 化的核心一环）。
//!
//! service 的业务状态变化（下载进度/状态、在线取流进度、登录 MFA 状态）
//! 统一在这里以 **强类型事件** 发布；Tauri 壳把事件映射成原名原载荷的
//! 前端事件（`hongguo-tauri/src/events.rs`），GPUI 壳转成自己的 UI 状态
//! 更新。service 不再接触任何具体 UI/事件机制、不再持有 `AppHandle`。
//!
//! 传输语义：tokio broadcast（多订阅者、容量 1024）。进度类事件已按
//! 0.5%/500ms 节流，容量足够吸收瞬时突发；订阅者滞后时丢弃旧事件并记
//! 日志——与原先「emit 尽力送达」的语义一致，壳侧列表数据另有快照查询
//! 兜底（队列状态等）。
//!
//! 契约锁定：事件名常量与载荷结构沿用各域既有定义（前端 zod 镜像），
//! 序列化形状由 `tests/wire_contract.rs` 快照守护，重构期间不得漂移。

use tokio::sync::broadcast;

use crate::download_service::events::{
    ProgressEvent, QueueChangedEvent, TaskEvent, TaskFailureEvent,
};
use crate::follow_service::events::FollowNoticesEvent;
use crate::login_service::events::LoginMfaStateEvent;
use crate::play_service::compat_play::CompatProgressEvent;
use crate::play_service::online::OnlineProgressEvent;
use hongguo_common::model::{DownloadTask, MergeTask};

/// 总线容量：进度事件已节流，1024 足够吸收瞬时突发。
const CAPACITY: usize = 1024;

/// service 对外的全部业务事件（闭集）。
#[derive(Debug, Clone)]
pub enum ServiceEvent {
    /// 下载任务入队（前端事件名 `download-task-added`，载荷 = 任务快照）。
    DownloadTaskAdded(DownloadTask),
    /// 单任务下载进度（`download-progress`）。
    DownloadProgress(ProgressEvent),
    /// 单任务完成（`download-completed`）。
    DownloadCompleted(TaskEvent),
    /// 单任务失败（`download-failed`）。
    DownloadFailed(TaskFailureEvent),
    /// 单任务停止（`download-stopped`）。
    DownloadStopped(TaskEvent),
    /// 队列整体变化（`download-queue-changed`，载荷空对象）。
    DownloadQueueChanged(QueueChangedEvent),
    /// 合并任务登记（`merge-task-added`，载荷 = 任务快照）。
    MergeTaskAdded(MergeTask),
    /// 合并进度（`merge-progress`，任务快照含 percent / episodeCount）。
    MergeProgress(MergeTask),
    /// 合并完成（`merge-completed`，任务快照含产物路径）。
    MergeCompleted(MergeTask),
    /// 合并失败（`merge-failed`，任务快照含 i18n error）。
    MergeFailed(MergeTask),
    /// 在线取流/解密进度（`online-play-progress`）。
    OnlineProgress(OnlineProgressEvent),
    /// 兼容转码进度（`compat-play-progress`）。
    CompatProgress(CompatProgressEvent),
    /// 登录 MFA 后台状态（`login-mfa-state`）。
    LoginMfa(LoginMfaStateEvent),
    /// 更新提醒有新条目（`follow-notices`，载荷 = 本次新增的通知）。
    FollowNotices(FollowNoticesEvent),
    /// 设备注册换档完成（载荷 = 新 iid）：启动自动注册/健康探针轮换成功
    /// 后发布。身份敏感的数据（顶栏 tab 等）收到后应重拉。
    DeviceRegistered(String),
}

/// 事件总线句柄（Clone 共享，AppState 持有）。
#[derive(Clone)]
pub struct ServiceEvents {
    tx: broadcast::Sender<ServiceEvent>,
}

impl Default for ServiceEvents {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceEvents {
    /// 建总线（AppState 构造时各建一条）。
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CAPACITY);
        Self { tx }
    }

    /// 发布一个事件。无订阅者时静默丢弃（「前端还没起来」同语义）。
    pub fn publish(&self, event: ServiceEvent) {
        let _ = self.tx.send(event);
    }

    /// 订阅事件流（壳在装配期订阅一次）。
    pub fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.tx.subscribe()
    }
}
