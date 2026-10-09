//! 同步门面：专用 DB 线程 + 任务队列。
//!
//! 为什么需要这层桥：业务代码（commands / services / scheduler）对存储的
//! 40 来处访问全是**同步**调用，而 turso 是异步 API。与其把 async 一路
//! 感染进调度器与 worker（它们刻意按「锁不跨 await」的铁律组织），不如
//! 让一个专职线程独占一个 current_thread runtime 串行执行所有 SQL——
//! 语义上等价于旧的 `RwLock<DataStore>`，但粒度细到单条语句。
//!
//! 线程生命周期：`Store` 是通道发送端的 `Arc`。最后一个 `Store` 克隆
//! drop 后通道关闭，线程跑完手头的活自然退出。
//!
//! 调用语义：门面方法**阻塞**调用方线程直到该批 SQL 完成。本地内嵌库
//! 的单条语句是微秒到毫秒级，与旧实现「写锁内做整文件 fsync」相比
//! 只快不慢；唯一要避免的是在一个事务里塞上万行——那种批量操作
//! （旧档导入）只在启动时发生一次。

use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::domain::model::settings::Settings;
use crate::domain::model::{DownloadTask, MergeTask, PlaybackPosition, Series};
use crate::error::{AppError, AppResult};
use crate::store::db::Db;
use crate::store::entity;

/// 一次投递到 DB 线程的活：拿一个连接克隆，跑一段异步逻辑。
type Job = Box<dyn FnOnce(Db) -> BoxFuture<'static, ()> + Send + 'static>;

/// 存储门面。克隆廉价（内部是 Arc 的通道端）。
#[derive(Clone)]
pub struct Store {
    tx: Arc<tokio::sync::mpsc::UnboundedSender<Job>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish()
    }
}

impl Store {
    /// 打开文件库（含 schema 迁移）。
    pub fn open(path: impl AsRef<std::path::Path>) -> AppResult<Self> {
        Self::spawn_open(path.as_ref().to_string_lossy().into_owned())
    }

    /// 内存库（测试与 `AppState::default`）。
    pub fn open_memory() -> AppResult<Self> {
        Self::spawn_open(":memory:".to_string())
    }

    fn spawn_open(path: String) -> AppResult<Self> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
        let (init_tx, init_rx) = std::sync::mpsc::channel::<AppResult<()>>();
        std::thread::Builder::new()
            .name("hongguo-db".into())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    let _ = init_tx.send(Err(AppError::StoreCorrupt(
                        "DB 线程 runtime 创建失败".into(),
                    )));
                    return;
                };
                // 打开 + 建表在通道就绪前完成，失败要让 open() 的调用方知道
                let db = match rt.block_on(Db::open(&path)) {
                    Ok(db) => db,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };
                let _ = init_tx.send(Ok(()));
                rt.block_on(async move {
                    while let Some(job) = rx.recv().await {
                        job(db.clone()).await;
                    }
                });
            })
            .map_err(|e| AppError::StoreCorrupt(format!("DB 线程启动失败: {e}")))?;
        init_rx
            .recv()
            .map_err(|_| AppError::StoreCorrupt("DB 线程初始化无响应".into()))??;
        Ok(Self { tx: Arc::new(tx) })
    }

    /// 投递一个活并阻塞等结果。通道断开（线程已退出）按存储故障上报。
    ///
    /// 闭包签名是显式 HRTB：实体层的 future 借用 `&Db`，其生命周期与本次
    /// 调用绑定，不能写成 `'static`。
    fn exec<T, F>(&self, f: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a Db) -> BoxFuture<'a, AppResult<T>> + Send + 'static,
    {
        let (result_tx, result_rx) = std::sync::mpsc::channel::<AppResult<T>>();
        let job: Job = Box::new(move |db: Db| {
            Box::pin(async move {
                let _ = result_tx.send(f(&db).await);
            })
        });
        self.tx
            .send(job)
            .map_err(|_| AppError::StoreCorrupt("DB 线程已退出".into()))?;
        result_rx
            .recv()
            .map_err(|_| AppError::StoreCorrupt("DB 线程无响应".into()))?
    }

    // ---------- settings ----------

    pub fn settings(&self) -> AppResult<Option<Settings>> {
        self.exec(|db| Box::pin(entity::settings(db)))
    }

    pub fn save_settings(&self, settings: &Settings) -> AppResult<()> {
        let s = settings.clone();
        self.exec(move |db| Box::pin(async move { entity::save_settings(db, &s).await }))
    }

    // ---------- tasks ----------

    pub fn tasks(&self) -> AppResult<Vec<DownloadTask>> {
        self.exec(|db| Box::pin(entity::tasks(db)))
    }

    /// 用队列快照整体镜像任务表（`persist_tasks` 的目标）。
    pub fn replace_tasks(&self, snapshot: &[DownloadTask]) -> AppResult<()> {
        let snap = snapshot.to_vec();
        self.exec(move |db| Box::pin(async move { entity::replace_tasks(db, &snap).await }))
    }

    // ---------- series ----------

    pub fn series_all(&self) -> AppResult<Vec<Series>> {
        self.exec(|db| Box::pin(entity::series_all(db)))
    }

    pub fn series_by_id(&self, series_id: &str) -> AppResult<Option<Series>> {
        let id = series_id.to_string();
        self.exec(move |db| Box::pin(async move { entity::series_by_id(db, &id).await }))
    }

    pub fn upsert_series(&self, series: &Series) -> AppResult<()> {
        let s = series.clone();
        self.exec(move |db| Box::pin(async move { entity::upsert_series(db, &s).await }))
    }

    // ---------- playback ----------

    pub fn playback_position(
        &self,
        series_id: &str,
        vid_index: u32,
    ) -> AppResult<Option<PlaybackPosition>> {
        let id = series_id.to_string();
        self.exec(move |db| {
            Box::pin(async move { entity::playback_position(db, &id, vid_index).await })
        })
    }

    /// 最热写路径：播放期间每 5 秒一次。
    pub fn save_playback_position(
        &self,
        series_id: &str,
        vid_index: u32,
        pos: &PlaybackPosition,
    ) -> AppResult<()> {
        let id = series_id.to_string();
        let p = *pos;
        self.exec(move |db| {
            Box::pin(async move { entity::save_playback_position(db, &id, vid_index, &p).await })
        })
    }

    /// 一部剧最近看到的那一集（详情页「继续看」的真值来源）。
    pub fn series_last_position(
        &self,
        series_id: &str,
    ) -> AppResult<Option<(u32, PlaybackPosition)>> {
        let id = series_id.to_string();
        self.exec(move |db| Box::pin(async move { entity::series_last_position(db, &id).await }))
    }

    // ---------- merge ----------

    pub fn merge_tasks(&self) -> AppResult<Vec<MergeTask>> {
        self.exec(|db| Box::pin(entity::merge_tasks(db)))
    }

    pub fn upsert_merge_task(&self, task: &MergeTask) -> AppResult<()> {
        let t = task.clone();
        self.exec(move |db| Box::pin(async move { entity::upsert_merge_task(db, &t).await }))
    }

    /// 返回 0 行时调用方应报 NotFound。
    pub fn delete_merge_task(&self, id: &str) -> AppResult<u64> {
        let id = id.to_string();
        self.exec(move |db| Box::pin(async move { entity::delete_merge_task(db, &id).await }))
    }

    // ---------- 设备档案 ----------

    pub fn device_profile(&self) -> AppResult<Option<crate::signer::device::DeviceProfile>> {
        self.exec(|db| Box::pin(entity::device_profile(db)))
    }

    pub fn save_device_profile(
        &self,
        profile: &crate::signer::device::DeviceProfile,
    ) -> AppResult<()> {
        let p = profile.clone();
        self.exec(move |db| Box::pin(async move { entity::save_device_profile(db, &p).await }))
    }

    /// 换档前把现役档案复制到备份行（见 [`entity::backup_device_profile`]）。
    pub fn backup_device_profile(&self) -> AppResult<bool> {
        self.exec(|db| Box::pin(entity::backup_device_profile(db)))
    }

    /// 读上一代备份档案（没有换过档返回 None）。
    pub fn device_profile_backup(&self) -> AppResult<Option<crate::signer::device::DeviceProfile>> {
        self.exec(|db| Box::pin(entity::device_profile_backup(db)))
    }

    /// 读 bootstrap 元数据（轮换来源、上次注册尝试；无则 None）。
    pub fn device_meta_get(&self) -> AppResult<Option<serde_json::Value>> {
        self.exec(|db| Box::pin(entity::device_meta_get(db)))
    }

    /// 写 bootstrap 元数据（整文档覆盖）。
    pub fn device_meta_set(&self, meta: &serde_json::Value) -> AppResult<()> {
        let meta = meta.clone();
        self.exec(move |db| Box::pin(async move { entity::device_meta_set(db, &meta).await }))
    }

    // ---------- 旧档迁移 ----------

    /// 导入旧 data.json 内容（幂等）。详见 [`entity::import_legacy`]。
    pub fn import_legacy(&self, legacy: &super::json_migrate::LegacyData) -> AppResult<()> {
        let data = legacy.clone();
        self.exec(move |db| Box::pin(async move { entity::import_legacy(db, &data).await }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_roundtrips_through_the_db_thread() {
        let store = Store::open_memory().expect("内存库");
        store.save_settings(&Settings::default()).expect("写设置");
        assert!(store.settings().expect("读设置").is_some());

        // 同一 Store 的克隆共享同一个 DB 线程
        let clone = store.clone();
        clone
            .save_playback_position("A", 1, &PlaybackPosition::new(1.0, 2.0))
            .expect("写进度");
        assert_eq!(
            store
                .playback_position("A", 1)
                .expect("读进度")
                .map(|p| p.current_time),
            Some(1.0)
        );
    }

    #[test]
    fn open_failure_reports_through_init_channel() {
        // 用「普通文件下的子路径」构造必然失败：建目录与 open 都得到
        // ENOTDIR/NOTFOUND 族错误。（原来用 Windows 保留字符做路径，在
        // macOS 上完全合法——测试假失败之外还会在工作目录里留下一个真库文件。）
        // 除报错本身外还钉死分类：这是环境问题不是损坏，必须走 Io 而不是
        // StoreCorrupt——「数据损坏」会引人去删库。
        let file = std::env::temp_dir().join(format!("hg-not-a-dir-{}", std::process::id()));
        std::fs::write(&file, b"x").expect("造一个普通文件");
        let bad = file.join("child.db");
        let err = Store::open(bad.to_str().expect("临时路径是 UTF-8"));
        match err {
            Err(AppError::Io(_)) => {}
            other => panic!("非目录路径必须报 Io（不是 StoreCorrupt）且不得带病运行: {other:?}"),
        }
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn open_creates_missing_parent_dirs_for_fresh_install() {
        // v0.1.0 回归：全新安装时数据目录整条不存在，Turso 的 build() 不建
        // 父目录，开库报 I/O entity not found 被映射成「数据损坏」直接
        // exit(1)——窗口未起即退，表现为装完即闪退。开库必须自建目录。
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("时钟正常")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("hg-fresh-install-{}-{nanos}", std::process::id()));
        let db_path = base.join("data/hongguo.db");
        {
            let store = Store::open(&db_path).expect("父目录缺失应自动创建并打开成功");
            store.save_settings(&Settings::default()).expect("写设置");
            assert!(
                store.settings().expect("读设置").is_some(),
                "建好的库应立即可用"
            );
        }
        assert!(db_path.is_file(), "库文件应已落盘");
        let _ = std::fs::remove_dir_all(&base);
    }
}
