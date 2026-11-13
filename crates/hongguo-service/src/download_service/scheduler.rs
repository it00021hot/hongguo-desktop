//! 下载调度器：后台驱动 worker（挑任务 → 按并发上限派发 → 收尾更新状态），单集执行在
//! [`crate::download_service::worker`]。
//! 到处是 owned：任务体跑在共用运行时的 `spawn` 里（future 必须 `Send`），而 parking_lot
//! guard 不是 `Send`——锁绝不跨 `.await`，只在同步块里取，跨 await 一律传 owned 值。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

use crate::app_state::AppState;
use crate::download_service::events::ProgressThrottle;
use crate::download_service::worker::{self, EpisodeDownload, ProgressSink};
use crate::events::ServiceEvent;
use hongguo_common::model::DownloadTask;
use hongguo_common::{AppError, AppResult};

/// 调度器。
pub struct DownloadScheduler {
    /// 一键暂停时置位，让在途任务尽快收尾（同时阻断新派发）
    cancel: Arc<AtomicBool>,
    /// 每任务取消位（单个停止用）：任务 id → 标志，任务收尾时移除；
    /// `cancel_task` 对尚未登记的任务先建后置位，盖住「已派发、未开跑」的窗口。
    flags: Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
    /// 正在运行的任务数
    running: Arc<Mutex<usize>>,
    throttle: Arc<ProgressThrottle>,
}

impl Default for DownloadScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadScheduler {
    /// 构造调度器。
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            flags: Mutex::new(std::collections::HashMap::new()),
            running: Arc::new(Mutex::new(0)),
            throttle: Arc::new(ProgressThrottle::default()),
        }
    }

    /// 一键暂停：置全局取消位，并让在途任务尽快收尾。
    pub fn pause_all(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        for flag in self.flags.lock().values() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// 一键启动：清取消位。
    pub fn resume_all(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }

    /// 是否已取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// 取消单个任务（真实取消：在途 worker 在下个分片检查点退出）。
    /// 对尚未登记的任务先建后置位——「已派发、未开跑」的窗口里标志会被 `execute_inner` 读到并按取消收尾。
    pub fn cancel_task(&self, id: &str) {
        self.flags
            .lock()
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .store(true, Ordering::SeqCst);
    }

    /// 调度一轮：把能跑的任务派出去。在「启动、提交、恢复」时调用；
    /// 每个任务完成时会自动再触发下一轮（见 `execute` 末尾的 `reschedule`）。
    pub async fn run_once(self: &Arc<Self>, state: AppState) {
        if self.is_cancelled() {
            return;
        }

        // 同步块：挑任务 + 占坑，全程不 await，锁的生命周期不会外溢
        let pending = self.claim_tasks(&state);

        for task in pending {
            let sched = Arc::clone(self);
            let state2 = Arc::clone(&state);
            crate::runtime::spawn(async move {
                sched.execute(state2, task).await;
            });
        }
    }

    /// 完成一个任务后重新调度。单独抽出来是为了断开递归类型：
    /// `execute` 不直接 `await` `run_once`，否则两者的 opaque future 会形成环，编译器无法确定大小。
    fn reschedule(self: &Arc<Self>, state: AppState) {
        let sched = Arc::clone(self);
        crate::runtime::spawn(async move {
            sched.run_once(state).await;
        });
    }

    /// 从队列挑出可派发的任务并标记为运行中。同步执行，返回 owned 任务列表。
    fn claim_tasks(&self, state: &AppState) -> Vec<DownloadTask> {
        let queue = state.queue();
        let limit = queue.limit();
        let mut out = Vec::new();

        for _ in 0..limit {
            if self.is_cancelled() {
                break;
            }
            if *self.running.lock() >= limit {
                break;
            }
            let Some(task) = queue.next_runnable() else {
                break;
            };
            if !queue.mark_running(&task.id) {
                continue; // 状态已被别的流程改掉，换下一个
            }
            *self.running.lock() += 1;
            out.push(task);
        }

        out
    }

    /// 执行单个任务并收尾。
    async fn execute(self: Arc<Self>, state: AppState, task: DownloadTask) {
        // 编译期自检：execute_inner 的 future 必须是 Send，回归时让报错精确指向这一行
        fn assert_send<T: Send>(_: &T) {}
        let fut = self.execute_inner(state.clone(), task);
        assert_send(&fut);
        fut.await;
    }

    async fn execute_inner(self: Arc<Self>, state: AppState, task: DownloadTask) {
        let id = task.id.clone();
        let queue = state.queue();
        // 取消位：worker 在分片边界读取；停止命令可能先到（先建后置位），entry 取现值不覆盖已有的 true。
        let flag = self
            .flags
            .lock()
            .entry(id.clone())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone();
        // 暂停发生在「已派发、未开跑」的窗口：不进入下载，直接按取消收尾
        if self.is_cancelled() {
            flag.store(true, Ordering::SeqCst);
        }
        let result = run_one(Arc::clone(&self), &state, &task, &flag).await;
        self.flags.lock().remove(&id);

        match result {
            Ok((path, size)) => {
                queue.mark_completed(&id, &path, size);
                self.throttle.forget(&id);
                state
                    .events()
                    .publish(ServiceEvent::DownloadCompleted(super::events::TaskEvent {
                        id: id.clone(),
                    }));
            }
            Err(e) => {
                if is_cancelled(&e) {
                    queue.mark_stopped(&id);
                    self.throttle.forget(&id);
                    state.events().publish(ServiceEvent::DownloadStopped(
                        super::events::TaskEvent { id },
                    ));
                } else {
                    // 队列里记 i18n key：DownloadTask.error 被前端直接渲染，底层细节只该进日志。
                    log::error!("[Download] 任务 {id} 失败: {e}");
                    queue.mark_failed(&id, e.i18n_key());
                    self.throttle.forget(&id);
                    state.events().publish(ServiceEvent::DownloadFailed(
                        super::events::TaskFailureEvent {
                            id,
                            error: e.to_string(),
                        },
                    ));
                }
            }
        }

        *self.running.lock() -= 1;
        state.events().publish(ServiceEvent::DownloadQueueChanged(
            super::events::QueueChangedEvent {},
        ));
        persist(&state);

        self.reschedule(state);
    }
}

/// 判断错误是不是「用户取消」。
fn is_cancelled(e: &AppError) -> bool {
    matches!(e, AppError::Cancelled)
}

/// 一集的完整流程：取流地址 → 下载 → 解密。
async fn run_one(
    sched: Arc<DownloadScheduler>,
    state: &AppState,
    task: &DownloadTask,
    cancel: &AtomicBool,
) -> AppResult<(String, u64)> {
    // 停止/暂停可能在派发后、取流前到达：不再发起任何网络请求
    if cancel.load(Ordering::SeqCst) {
        return Err(AppError::Cancelled);
    }
    let settings = state.settings();

    // 下载固定取最高档：清晰度切换是播放时的交互，下载侧没有这个概念；
    // 选流模式保持 Full（下载件不做解码能力翻转），下载件的播放走既有 compat 兜底。
    crate::cold_start::wait(state).await?;
    let play = hongguo_api::play_url::fetch_play_url(
        &task.vid,
        None,
        hongguo_api::stream_pick::DecoderCapability::full(),
        &state.api_env(),
    )
    .await
    .map_err(|e| AppError::Network(e.to_string()))?;

    let series_title = state
        .store
        .series_by_id(&task.series_id)
        .ok()
        .flatten()
        .map(|s| s.title)
        .unwrap_or_else(|| task.series_title.clone());
    let file_name = settings.render_file_name(&task.series_title, task.vid_index, &task.ep_title);
    let output = settings
        .series_dir(&series_title)
        .join(format!("{file_name}.mp4"));

    let episode = EpisodeDownload {
        id: task.id.clone(),
        video_url: play.url,
        output_path: output.clone(),
        encrypted: play.encrypted,
        key_material: play.key_material,
        settings,
        cancelled: cancel,
    };

    // 进度回调：在闭包内部取 queue，避免 guard 跨越 await
    let queue = state.queue();
    let task_id = task.id.clone();
    let events = state.events().clone();
    let on_progress = move |downloaded: u64, total: u64| {
        queue.update_progress(&task_id, downloaded, total);
        // 百分比只有 DownloadTask::percent 一个口径，不在调度器里再算一遍。
        let percent = queue.get(&task_id).map(|t| t.percent()).unwrap_or(0.0);
        events.publish(ServiceEvent::DownloadProgress(
            super::events::ProgressEvent {
                id: task_id.clone(),
                downloaded,
                total,
                percent,
            },
        ));
    };
    let sink = ProgressSink {
        throttle: Arc::clone(&sched.throttle),
        on_progress: Arc::new(on_progress),
    };

    let size = worker::download_episode(&episode, &sink).await?;
    Ok((output.to_string_lossy().to_string(), size))
}

/// 把队列写回磁盘。
fn persist(state: &AppState) {
    // 调度器跑在后台任务里，没有调用方能接住错误；落盘失败已由 persist_tasks 记进日志。
    let _ = crate::download_service::persist_tasks(&state.store, &state.queue(), "Download");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download_service::queue::DownloadQueue;

    #[test]
    fn cancel_flag_round_trips() {
        let s = DownloadScheduler::new();
        assert!(!s.is_cancelled());
        s.pause_all();
        assert!(s.is_cancelled());
        s.resume_all();
        assert!(!s.is_cancelled());
    }

    #[test]
    fn cancel_flag_shared_between_clones() {
        let s = Arc::new(DownloadScheduler::new());
        let c = Arc::clone(&s);
        c.cancel.store(true, Ordering::SeqCst);
        assert!(s.is_cancelled(), "克隆应共享同一标志");
    }

    #[test]
    fn cancel_task_survives_the_dispatch_window() {
        let s = DownloadScheduler::new();
        s.cancel_task("t1");
        let flag = s
            .flags
            .lock()
            .entry("t1".into())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone();
        assert!(flag.load(Ordering::SeqCst), "登记不得把停止位覆盖掉");
    }

    #[test]
    fn pause_all_cancels_in_flight_tasks() {
        let s = DownloadScheduler::new();
        let flag = Arc::new(AtomicBool::new(false));
        s.flags.lock().insert("t".into(), Arc::clone(&flag));
        s.pause_all();
        assert!(flag.load(Ordering::SeqCst), "在途任务的取消位要被暂停置位");
    }

    #[test]
    fn cancelled_error_is_recognized() {
        assert!(is_cancelled(&AppError::Cancelled));
        assert!(!is_cancelled(&AppError::Io("磁盘满了".into())));
        assert!(!is_cancelled(&AppError::Network("超时".into())));
    }

    #[test]
    fn claim_returns_empty_when_queue_empty() {
        let s = DownloadScheduler::new();
        let state = AppState::default();
        assert!(s.claim_tasks(&state).is_empty());
    }

    #[test]
    fn raising_limit_lets_claim_fill_new_slots() {
        let s = DownloadScheduler::new();
        let state = AppState::default();
        let queue = state.queue();
        queue.set_limit(2);
        for i in 0..5 {
            queue.enqueue(DownloadTask::new(&i.to_string(), "剧", i, "v", "第1集"));
        }
        assert_eq!(s.claim_tasks(&state).len(), 2, "旧上限下只派 2 个");
        queue.set_limit(4);
        assert_eq!(
            s.claim_tasks(&state).len(),
            2,
            "一轮把新坑补满到新上限（2 在途 + 2 新派 = 4）"
        );
        assert!(s.claim_tasks(&state).is_empty(), "满编后不再多派");
    }

    #[test]
    fn failed_task_records_i18n_key_not_internal_detail() {
        let queue = DownloadQueue::new();
        let t = queue.enqueue(DownloadTask::new("1", "剧", 1, "v", "第1集"));
        let e = AppError::Network("dns 解析失败".into());

        queue.mark_failed(&t.id, e.i18n_key());

        let stored = &queue.get(&t.id).expect("任务还在队列里").error;
        assert_eq!(stored, "error.network", "存进任务的应是可翻译的 key");
        assert!(
            !stored.contains("dns"),
            "底层细节只该进日志，不该糊在界面上: {stored}"
        );
    }
}
