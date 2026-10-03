//! 全局应用状态。
//!
//! 只持有跨模块共享的句柄，不放业务逻辑。设置与队列的并发控制用
//! `parking_lot` 的读写锁，避免在 async 上下文里跨 await 持锁；
//! 持久化则交给 [`Store`]（专职 DB 线程，天然串行，不需要外层锁）。

use std::sync::Arc;

use parking_lot::RwLock;

use crate::domain::model::settings::Settings;
use crate::service::download_service::queue::DownloadQueue;
use crate::service::download_service::scheduler::DownloadScheduler;
use crate::store::Store;

/// 全局状态，由 Tauri 的 `manage` 注入。
///
/// 用 `Arc<AppState>` 形式挂到 Tauri，这样 command 层既能拿 `State<'_, Arc<AppState>>`
/// 的借用，也能在需要 `'static` 所有权时（如 spawn 后台任务）直接克隆。
pub struct AppStateInner {
    /// 持久化数据（Turso 内嵌库，DB 线程串行执行）
    pub store: Store,
    /// 当前设置（改后立即生效，无需重启）
    pub settings: RwLock<Settings>,
    /// 当前设备档案（设备注册成功后被整体替换）
    device: RwLock<crate::signer::device::DeviceProfile>,
    /// 下载队列
    pub queue: RwLock<Arc<DownloadQueue>>,
    scheduler: RwLock<Arc<DownloadScheduler>>,
}

/// 对外暴露的句柄类型。`Arc` 的 `Default` 由 std 提供，此处只需给内层实现。
pub type AppState = Arc<AppStateInner>;

impl AppStateInner {
    /// 用真实文件库构造（应用启动用）。打开失败（建不了库/迁移失败）
    /// 直接终止启动：带病运行等于让用户在新库上改设置，回头又换回旧库。
    pub fn with_db(store: Store) -> Self {
        Self::with_device(store, crate::signer::video_device())
    }

    /// [`AppStateInner::with_db`] 的带设备版本（启动装配时传入库里
    /// 装载好的档案；测试与默认路径用静态兜底档案）。
    pub fn with_device(store: Store, device: crate::signer::device::DeviceProfile) -> Self {
        Self {
            store,
            settings: RwLock::new(Settings::default()),
            device: RwLock::new(device),
            queue: RwLock::new(Arc::new(DownloadQueue::new())),
            scheduler: RwLock::new(Arc::new(DownloadScheduler::new())),
        }
    }
}

impl Default for AppStateInner {
    fn default() -> Self {
        // 测试路径：内存库。打不开属于环境异常，直接 panic 让测试显式失败。
        let store = Store::open_memory().expect("测试内存库初始化失败");
        Self::with_db(store)
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

    /// 当前设备档案快照。
    pub fn device(&self) -> crate::signer::device::DeviceProfile {
        self.device.read().clone()
    }

    /// 整体替换设备档案（设备注册成功时）。
    // M2b 设备注册落位前的脚手架。
    #[allow(dead_code)]
    pub fn replace_device(&self, next: crate::signer::device::DeviceProfile) {
        *self.device.write() = next;
    }

    /// 一次 API 调用所需的完整环境快照：代理 + 设备 + 会话 Cookie。
    ///
    /// 快照语义是刻意的：一次调用链内环境不变，避免「签名用 A 设备、
    /// 请求带 B Cookie」的半新半旧。登录态（M3）接入后 Cookie 从这里透出。
    pub fn api_env(&self) -> crate::domain::api::client::ApiEnv {
        crate::domain::api::client::ApiEnv {
            proxy: self.settings().proxy,
            device: self.device(),
            cookie: None,
        }
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

    #[test]
    fn store_is_shared_and_writable() {
        let state = AppState::default();
        state
            .store
            .save_playback_position("A", 1, &crate::domain::model::PlaybackPosition::new(1.0, 2.0))
            .expect("内存库写入");
        assert_eq!(
            state
                .store
                .playback_position("A", 1)
                .expect("内存库读取")
                .map(|p| p.current_time),
            Some(1.0)
        );
    }
}
