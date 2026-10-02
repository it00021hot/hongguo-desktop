//! 全局应用状态。
//!
//! 只持有跨模块共享的句柄，不放业务逻辑。并发控制用 `parking_lot` 的读写锁，
//! 避免在 async 上下文里跨 await 持锁。

use std::sync::Arc;

use parking_lot::RwLock;

use crate::domain::model::settings::Settings;
use crate::service::download_service::queue::DownloadQueue;
use crate::service::download_service::scheduler::DownloadScheduler;
use crate::store::DataStore;

/// 全局状态，由 Tauri 的 `manage` 注入。
///
/// 用 `Arc<AppState>` 形式挂到 Tauri，这样 command 层既能拿 `State<'_, Arc<AppState>>`
/// 的借用，也能在需要 `'static` 所有权时（如 spawn 后台任务）直接克隆。
pub struct AppStateInner {
    /// 持久化数据
    pub store: RwLock<DataStore>,
    /// 当前设置（改后立即生效，无需重启）
    pub settings: RwLock<Settings>,
    /// 下载队列
    pub queue: RwLock<Arc<DownloadQueue>>,
    /// 下载调度器
    scheduler: RwLock<Arc<DownloadScheduler>>,
}

/// 对外暴露的句柄类型。`Arc` 的 `Default` 由 std 提供，此处只需给内层实现。
pub type AppState = Arc<AppStateInner>;

impl Default for AppStateInner {
    fn default() -> Self {
        Self {
            store: RwLock::new(DataStore::empty()),
            settings: RwLock::new(Settings::default()),
            queue: RwLock::new(Arc::new(DownloadQueue::new())),
            scheduler: RwLock::new(Arc::new(DownloadScheduler::new())),
        }
    }
}

impl AppStateInner {
    /// 取当前下载队列句柄。
    pub fn queue(&self) -> Arc<DownloadQueue> {
        self.queue.read().clone()
    }

    /// 替换下载队列（启动时从持久化数据恢复）。
    pub fn replace_queue(&self, queue: DownloadQueue) {
        *self.queue.write() = Arc::new(queue);
    }

    /// 取调度器句柄。
    pub fn scheduler(&self) -> Arc<DownloadScheduler> {
        self.scheduler.read().clone()
    }

    /// 取当前设置快照。
    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    /// 覆盖当前设置，返回旧值供代理 client 重建使用。
    pub fn replace_settings(&self, next: Settings) -> Settings {
        let mut guard = self.settings.write();
        std::mem::replace(&mut *guard, next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_snapshot_is_a_copy() {
        let state = AppState::default();
        let mut s = state.settings();
        s.max_concurrency = 7;
        // 改副本不应影响全局
        assert_eq!(state.settings().max_concurrency, 3);
    }

    #[test]
    fn replace_settings_returns_old() {
        let state = AppState::default();
        let old = state.replace_settings(Settings {
            max_concurrency: 9,
            ..Settings::default()
        });
        assert_eq!(old.max_concurrency, 3);
        assert_eq!(state.settings().max_concurrency, 9);
    }

    #[test]
    fn queue_handle_is_shared() {
        let state = AppState::default();
        let a = state.queue();
        let b = state.queue();
        a.enqueue(crate::domain::model::DownloadTask::new(
            "1", "t", 1, "v", "",
        ));
        assert_eq!(b.all().len(), 1, "多次获取应指向同一队列");
    }

    #[test]
    fn scheduler_handle_is_shared() {
        let state = AppState::default();
        let a = state.scheduler();
        let b = state.scheduler();
        assert!(Arc::ptr_eq(&a, &b), "多次获取应指向同一调度器");
    }
}
