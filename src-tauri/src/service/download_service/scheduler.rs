//! 下载调度器：后台驱动 worker。
//!
//! 职责：从队列挑可运行任务 → 按并发上限派发 → 收尾更新状态。
//! 单集的具体执行在 [`crate::service::download_service::worker`]。
//!
//! ## 为什么到处是 owned
//!
//! 任务体跑在 `tauri::async_runtime::spawn` 里，要求整个 future 是 `Send`。
//! 本模块大量使用 `parking_lot` 的锁，其 guard **不是 `Send`**——只要 guard
//! 存活跨越 `.await`，编译器就会拒绝。因此：
//!
//! - 锁只在同步块里取，取完立刻把需要的数据 clone 出来
//! - 跨 `.await` 传递的一律用 owned 值（`Arc`、`String`、`PathBuf`）
//! - 进度回调用 `Arc<dyn Fn + Send + Sync>` 持有，不借用栈上的闭包

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

use crate::app_state::AppState;
use crate::domain::model::DownloadTask;
use crate::error::{AppError, AppResult};
use crate::service::download_service::events::{names, ProgressThrottle};
use crate::service::download_service::worker::{self, EpisodeDownload, ProgressSink};

/// 调度器。
pub struct DownloadScheduler {
    /// 一键暂停时置位，让在途任务尽快收尾
    cancel: Arc<AtomicBool>,
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
            running: Arc::new(Mutex::new(0)),
            throttle: Arc::new(ProgressThrottle::default()),
        }
    }

    /// 一键暂停：置取消位。
    pub fn pause_all(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// 一键启动：清取消位。
    pub fn resume_all(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }

    /// 是否已取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// 调度一轮：把能跑的任务派出去。
    ///
    /// 在「启动、提交、恢复」时调用；每个任务完成时会自动再触发下一轮
    /// （见 [`Self::execute`] 末尾的 [`Self::reschedule`]）。
    pub async fn run_once(self: &Arc<Self>, app: AppHandle, state: AppState) {
        if self.is_cancelled() {
            return;
        }

        // 同步块：挑任务 + 占坑，全程不 await，锁的生命周期不会外溢
        let pending = self.claim_tasks(&state);

        for task in pending {
            let sched = Arc::clone(self);
            let app2 = app.clone();
            let state2 = Arc::clone(&state);
            tauri::async_runtime::spawn(async move {
                sched.execute(app2, state2, task).await;
            });
        }
    }

    /// 完成一个任务后重新调度。
    ///
    /// 单独抽出来是为了断开递归类型：`execute` 不直接 `await` `run_once`，
    /// 否则两者的 opaque future 会形成环，编译器无法确定大小。
    fn reschedule(self: &Arc<Self>, app: AppHandle, state: AppState) {
        let sched = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            sched.run_once(app, state).await;
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
    async fn execute(self: Arc<Self>, app: AppHandle, state: AppState, task: DownloadTask) {
        // 编译期自检：断言 execute_inner 的 future 是 Send，
        // 这样一旦某天又引入非 Send 的持有物，报错会精确指向这一行，
        // 而不是外层 spawn 那行。
        fn assert_send<T: Send>(_: &T) {}
        let fut = self.execute_inner(app.clone(), state.clone(), task);
        assert_send(&fut);
        fut.await;
    }

    async fn execute_inner(self: Arc<Self>, app: AppHandle, state: AppState, task: DownloadTask) {
        let id = task.id.clone();
        let queue = state.queue();
        let result = run_one(Arc::clone(&self), &app, &state, &task).await;

        match result {
            Ok((path, size)) => {
                queue.mark_completed(&id, &path, size);
                self.throttle.forget(&id);
                let _ = app.emit(names::DOWNLOAD_COMPLETED, &serde_json::json!({ "id": id }));
            }
            Err(e) => {
                if is_cancelled(&e) {
                    queue.mark_stopped(&id);
                    self.throttle.forget(&id);
                    let _ = app.emit(names::DOWNLOAD_STOPPED, &serde_json::json!({ "id": id }));
                } else {
                    // 队列里记 i18n key，不记「网络请求失败: dns 错误」这种人话：
                    // `DownloadTask.error` 是被前端直接渲染的，底层细节只该进日志。
                    log::error!("[Download] 任务 {id} 失败: {e}");
                    queue.mark_failed(&id, e.i18n_key());
                    self.throttle.forget(&id);
                    let _ = app.emit(
                        names::DOWNLOAD_FAILED,
                        &serde_json::json!({ "id": id, "error": e.to_string() }),
                    );
                }
            }
        }

        *self.running.lock() -= 1;
        let _ = app.emit(names::DOWNLOAD_QUEUE_CHANGED, &serde_json::json!({}));
        persist(&state);

        // 继续派发下一个（走 reschedule 断开递归类型）
        self.reschedule(app, state);
    }
}

/// 判断错误是不是「用户取消」。
fn is_cancelled(e: &AppError) -> bool {
    matches!(e, AppError::Cancelled)
}

/// 一集的完整流程：取流地址 → 下载 → 解密。
async fn run_one(
    sched: Arc<DownloadScheduler>,
    app: &AppHandle,
    state: &AppState,
    task: &DownloadTask,
) -> AppResult<(String, u64)> {
    let settings = state.settings();

    // 1) 取流地址（签名 + 清晰度择优）
    // 下载固定取最高档：清晰度切换是播放时的交互，下载侧没有这个概念
    let play = crate::domain::api::play_url::fetch_play_url(&task.vid, None, &settings.proxy)
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    // 2) 输出路径（档案查询是单条 SQL，同步快查即回）
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

    // 3) 下载
    let episode = EpisodeDownload {
        id: task.id.clone(),
        video_url: play.url,
        output_path: output.clone(),
        encrypted: play.encrypted,
        key_material: play.key_material,
        settings,
        cancelled: &sched.cancel,
    };

    // 进度回调：在闭包内部取 queue，避免 guard 跨越 await
    let queue = state.queue();
    let task_id = task.id.clone();
    let app = app.clone();
    let on_progress = move |downloaded: u64, total: u64| {
        queue.update_progress(&task_id, downloaded, total);
        // 百分比只有 [`DownloadTask::percent`] 一个口径：调度器自己再算一遍
        // 的话，clamp 之类的修正迟早会只落在一处
        let percent = queue.get(&task_id).map(|t| t.percent()).unwrap_or(0.0);
        let _ = app.emit(
            names::DOWNLOAD_PROGRESS,
            &serde_json::json!({
                "id": task_id,
                "downloaded": downloaded,
                "total": total,
                "percent": percent,
            }),
        );
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
    // 调度器跑在后台任务里，没有调用方能接住错误；
    // 落盘失败已由 `persist_tasks` 记进日志，这里只需让它在这里终止。
    let _ = crate::store::persist_tasks(state, "Download");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::download_service::queue::DownloadQueue;

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
    fn failed_task_records_i18n_key_not_internal_detail() {
        // 模拟一次失败：调度器落进队列的就是 `e.i18n_key()`，
        // 前端渲染的 DownloadTask.error 因此是 `error.*` 而不是中文串
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
