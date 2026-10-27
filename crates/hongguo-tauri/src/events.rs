//! service 事件 → 前端事件的转发适配器。
//!
//! service 经 `hongguo_service::events::ServiceEvent` 发布业务事件；本模块
//! 订阅一次并逐条映射为**事件名 + 载荷**原样 `app.emit`——事件名与载荷结构
//! 与前端 `service/tauri/types.ts` 的 EVENTS / zod schema 互为镜像（红线 3），
//! 改任何一侧必须同 commit 改另一侧。
//!
//! 订阅须在 bootstrap 阶段建立（早于启动期就可能发布事件的下载恢复）。

use hongguo_service::app_state::AppState;
use hongguo_service::download_service::events::names;
use hongguo_service::events::ServiceEvent;
use tauri::Emitter;
use tokio::sync::broadcast::error::RecvError;

/// 订阅 service 事件并把它们转发为前端事件（每次调用新起一条转发任务）。
pub fn subscribe(app: &tauri::AppHandle, state: &AppState) {
    let app = app.clone();
    let mut rx = state.events().subscribe();
    hongguo_service::runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => forward(&app, event),
                Err(RecvError::Lagged(skipped)) => {
                    // 高频进度事件滞后：丢弃旧事件即可（壳侧列表另有快照查询兜底）
                    log::warn!("[Events] 前端事件转发滞后，丢弃 {skipped} 条旧事件");
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// 事件 → (名, 载荷) 映射表。
///
/// 新增事件类型必须在这里登记，并把事件名同步到前端 `EVENTS`。
fn forward(app: &tauri::AppHandle, event: ServiceEvent) {
    match event {
        ServiceEvent::DownloadTaskAdded(task) => {
            let _ = app.emit(names::DOWNLOAD_TASK_ADDED, task);
        }
        ServiceEvent::DownloadProgress(ev) => {
            let _ = app.emit(names::DOWNLOAD_PROGRESS, ev);
        }
        ServiceEvent::DownloadCompleted(ev) => {
            let _ = app.emit(names::DOWNLOAD_COMPLETED, ev);
        }
        ServiceEvent::DownloadFailed(ev) => {
            let _ = app.emit(names::DOWNLOAD_FAILED, ev);
        }
        ServiceEvent::DownloadStopped(ev) => {
            let _ = app.emit(names::DOWNLOAD_STOPPED, ev);
        }
        ServiceEvent::DownloadQueueChanged(ev) => {
            let _ = app.emit(names::DOWNLOAD_QUEUE_CHANGED, ev);
        }
        ServiceEvent::MergeTaskAdded(task) => {
            let _ = app.emit(names::MERGE_TASK_ADDED, task);
        }
        ServiceEvent::MergeProgress(task) => {
            let _ = app.emit(names::MERGE_PROGRESS, task);
        }
        ServiceEvent::MergeCompleted(task) => {
            let _ = app.emit(names::MERGE_COMPLETED, task);
        }
        ServiceEvent::MergeFailed(task) => {
            let _ = app.emit(names::MERGE_FAILED, task);
        }
        ServiceEvent::OnlineProgress(ev) => {
            let _ = app.emit(names::ONLINE_PROGRESS, ev);
        }
        ServiceEvent::CompatProgress(ev) => {
            let _ = app.emit(names::COMPAT_PROGRESS, ev);
        }
        ServiceEvent::LoginMfa(ev) => {
            let _ = app.emit(hongguo_service::login_service::events::LOGIN_MFA_STATE, ev);
        }
        ServiceEvent::FollowNotices(ev) => {
            let _ = app.emit(hongguo_service::follow_service::events::FOLLOW_NOTICES, ev);
        }
        ServiceEvent::DeviceRegistered(iid) => {
            let _ = app.emit(hongguo_service::device::DEVICE_REGISTERED, iid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_service::download_service::events::{
        ProgressEvent, QueueChangedEvent, TaskEvent, TaskFailureEvent,
    };
    use hongguo_service::login_service::events::LoginMfaStateEvent;
    use hongguo_service::play_service::online::OnlineProgressEvent;

    /// 事件映射表守护：每个 ServiceEvent 变体都有确定的前端事件名
    /// （名字集与前端 `EVENTS` 一一对应，改一侧必须同 commit 改另一侧）。
    #[test]
    fn event_names_are_stable() {
        let pairs: Vec<(&str, ServiceEvent)> = vec![
            (
                names::DOWNLOAD_TASK_ADDED,
                ServiceEvent::DownloadTaskAdded(hongguo_common::model::DownloadTask::new(
                    "1", "剧", 1, "v", "",
                )),
            ),
            (
                names::DOWNLOAD_PROGRESS,
                ServiceEvent::DownloadProgress(ProgressEvent {
                    id: "t".into(),
                    downloaded: 1,
                    total: 2,
                    percent: 50.0,
                }),
            ),
            (
                names::DOWNLOAD_COMPLETED,
                ServiceEvent::DownloadCompleted(TaskEvent { id: "t".into() }),
            ),
            (
                names::DOWNLOAD_FAILED,
                ServiceEvent::DownloadFailed(TaskFailureEvent {
                    id: "t".into(),
                    error: "error.network".into(),
                }),
            ),
            (
                names::DOWNLOAD_STOPPED,
                ServiceEvent::DownloadStopped(TaskEvent { id: "t".into() }),
            ),
            (
                names::DOWNLOAD_QUEUE_CHANGED,
                ServiceEvent::DownloadQueueChanged(QueueChangedEvent {}),
            ),
            (
                names::MERGE_TASK_ADDED,
                ServiceEvent::MergeTaskAdded(hongguo_common::model::MergeTask::new(
                    "1",
                    "剧",
                    "out",
                    hongguo_common::model::MergeMode::Quick,
                )),
            ),
            (
                names::MERGE_PROGRESS,
                ServiceEvent::MergeProgress(hongguo_common::model::MergeTask::new(
                    "1",
                    "剧",
                    "out",
                    hongguo_common::model::MergeMode::Quick,
                )),
            ),
            (
                names::MERGE_COMPLETED,
                ServiceEvent::MergeCompleted(hongguo_common::model::MergeTask::new(
                    "1",
                    "剧",
                    "out",
                    hongguo_common::model::MergeMode::Quick,
                )),
            ),
            (
                names::MERGE_FAILED,
                ServiceEvent::MergeFailed(hongguo_common::model::MergeTask::new(
                    "1",
                    "剧",
                    "out",
                    hongguo_common::model::MergeMode::Quick,
                )),
            ),
            (
                names::ONLINE_PROGRESS,
                ServiceEvent::OnlineProgress(OnlineProgressEvent {
                    key: "1:1".into(),
                    received: 0,
                    total: 0,
                    percent: 100.0,
                    phase: "ready".into(),
                }),
            ),
            (
                names::COMPAT_PROGRESS,
                ServiceEvent::CompatProgress(
                    hongguo_service::play_service::compat_play::CompatProgressEvent {
                        key: "1:1".into(),
                        percent: 0.0,
                        phase: hongguo_service::play_service::compat_play::CompatPhase::Downloading,
                    },
                ),
            ),
            (
                hongguo_service::login_service::events::LOGIN_MFA_STATE,
                ServiceEvent::LoginMfa(LoginMfaStateEvent::Success { name: "甲".into() }),
            ),
            (
                hongguo_service::follow_service::events::FOLLOW_NOTICES,
                ServiceEvent::FollowNotices(
                    hongguo_service::follow_service::events::FollowNoticesEvent {
                        notices: Vec::new(),
                    },
                ),
            ),
            (
                hongguo_service::device::DEVICE_REGISTERED,
                ServiceEvent::DeviceRegistered("2715158266282762".into()),
            ),
        ];
        // 映射表在 forward() 里；这里锁定「一个变体一个名字」的名字集，
        // 防止将来加变体时只加事件不登记（前端订阅会静默失效）。
        let names: Vec<&str> = pairs.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "download-task-added",
                "download-progress",
                "download-completed",
                "download-failed",
                "download-stopped",
                "download-queue-changed",
                "merge-task-added",
                "merge-progress",
                "merge-completed",
                "merge-failed",
                "online-play-progress",
                "compat-play-progress",
                "login-mfa-state",
                "follow-notices",
                "device-registered",
            ]
        );
    }
}
