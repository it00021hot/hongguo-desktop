//! 内嵌 Turso 数据库：连接与 schema 迁移（`turso` 纯 Rust 无 C 工具链、`Builder::new_local` 本地内嵌；刻意关闭 default features——mimalloc 是全局分配器，不能让库替我们换）。
//!
//! 并发模型：`Connection` 内部走连接级串行，多个 `connect()` 共享同一文件由库自身做页级加锁，无需外层 `RwLock`。事务边界：`begin/commit/rollback` 是独立 SQL 语句而非持有式 API，调用方必须在同一 async 任务里成对使用，仓储层 `with_tx` 收口。

use std::path::Path;

use turso::{Builder, Connection, Database};

use hongguo_common::{AppError, AppResult};

/// 当前 schema 版本。每次结构性变更（加表/加列）时 +1，
/// 并在 [`MIGRATIONS`] 追加一段从上一版本到新版本的 SQL。
const SCHEMA_VERSION: i64 = 8;

/// 按版本升序排列的迁移 SQL，第 n 段把 schema 从版本 n 升到 n+1。
pub static MIGRATIONS: &[&str] = &[
    // v0 -> v1：初始五张表——复杂实体（任务/剧集/合并）整行存 JSON 列（serde 模型与 alias 兼容逻辑复用），
    // 只把需要 WHERE/ORDER 的字段提升为列；playback 最热写入点，全真实列 + UPSERT；settings 单行表（id=1）。
    r#"
    CREATE TABLE settings (
        id    INTEGER PRIMARY KEY CHECK (id = 1),
        json  TEXT NOT NULL
    );
    CREATE TABLE tasks (
        id         TEXT PRIMARY KEY,
        series_id  TEXT NOT NULL,
        status     TEXT NOT NULL,
        json       TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX idx_tasks_series ON tasks(series_id);
    CREATE TABLE series (
        series_id  TEXT PRIMARY KEY,
        dismissed  INTEGER NOT NULL DEFAULT 0,
        json       TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE playback (
        series_id    TEXT NOT NULL,
        vid_index    INTEGER NOT NULL,
        current_time REAL NOT NULL,
        duration     REAL NOT NULL,
        updated_at   INTEGER NOT NULL,
        PRIMARY KEY (series_id, vid_index)
    );
    CREATE INDEX idx_playback_updated ON playback(updated_at);
    CREATE TABLE merge_tasks (
        id         TEXT PRIMARY KEY,
        json       TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    "#,
    // v1 -> v2：device_profile 单行表（设备注册档案整份 JSON，启动装载）与 session 单行表（登录态 Cookie/用户资料，先建表占位）。
    r#"
    CREATE TABLE device_profile (
        id   INTEGER PRIMARY KEY CHECK (id = 1),
        json TEXT NOT NULL
    );
    CREATE TABLE session (
        id         INTEGER PRIMARY KEY CHECK (id = 1),
        cookies    TEXT NOT NULL DEFAULT '',
        user_json  TEXT,
        updated_at INTEGER NOT NULL
    );
    "#,
    // v2 -> v3：webp 封面缓存（信息流 HEIC 封面 WebView2 渲染不了，按 series_id 一剧一抓）。
    r#"
    CREATE TABLE cover_cache (
        series_id  TEXT PRIMARY KEY,
        cover      TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    "#,
    // v3 -> v4：官网链路整体下线（数据一律走官方 App 接口），webp 封面缓存作废——HEIC 封面改由本地 hongguo-cover 协议现转。
    r#"
    DROP TABLE IF EXISTS cover_cache;
    "#,
    // v4 -> v5：设备档案从单行扩成多行（1=现役，2=上一代备份，10=bootstrap 元数据）；
    // 老表的 CHECK (id = 1) 锁死单行，SQLite 不能删约束，按惯例重建表搬数据。
    r#"
    CREATE TABLE device_profile_new (
        id   INTEGER PRIMARY KEY,
        json TEXT NOT NULL
    );
    INSERT INTO device_profile_new (id, json)
        SELECT id, json FROM device_profile;
    DROP TABLE device_profile;
    ALTER TABLE device_profile_new RENAME TO device_profile;
    "#,
    // v5 -> v6：追更两件套——follow_baseline 记上次检查时服务端报的总集数（对照判定出新集，首次见到不提示），
    // follow_notice 存更新提醒（kind 留 TEXT 为预约上线提醒留位）；取消追更时基线行随删（重追从头计基线）。
    r#"
    CREATE TABLE follow_baseline (
        series_id   TEXT PRIMARY KEY,
        episode_cnt INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
    );
    CREATE TABLE follow_notice (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        kind      TEXT NOT NULL,
        series_id TEXT NOT NULL,
        title     TEXT NOT NULL,
        detail    TEXT NOT NULL,
        at        INTEGER NOT NULL,
        read      INTEGER NOT NULL DEFAULT 0
    );
    "#,
    // v6 -> v7：本地观看历史（未登录可记录/管理，登录后补传云端）——每剧一行（行 = 最近看的那集），
    // reported 标记该行是否已上报云端（0=待补传）。
    r#"
    CREATE TABLE local_history (
        series_id     TEXT PRIMARY KEY,
        vid_index     INTEGER NOT NULL,
        vid           TEXT NOT NULL DEFAULT '',
        title         TEXT NOT NULL DEFAULT '',
        cover         TEXT NOT NULL DEFAULT '',
        episode_count INTEGER NOT NULL DEFAULT 0,
        position_ms   INTEGER NOT NULL DEFAULT 0,
        duration_ms   INTEGER NOT NULL DEFAULT 0,
        updated_at    INTEGER NOT NULL,
        reported      INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_local_history_updated ON local_history(updated_at);
    "#,
    // v7 -> v8：官方非进程级清晰度记忆。回滚 SQL：DROP TABLE playback_definition;
    r#"
    CREATE TABLE playback_definition (
        scope TEXT PRIMARY KEY,
        definition INTEGER NOT NULL
    );
    "#,
];

/// 数据库句柄。`Database` 持有 IO 线程必须存活，`Connection` 可克隆共享。
#[derive(Clone)]
pub struct Db {
    conn: Connection,
}

impl Db {
    /// 打开（或创建）数据库文件并跑到最新 schema；传入 `":memory:"` 得到内存库（测试用）。
    pub async fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = if path.as_ref() == Path::new(":memory:") {
            ":memory:".to_string()
        } else {
            let open_at = path.as_ref();
            if let Some(dir) = open_at.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| AppError::Io(format!("创建数据目录失败: {e}")))?;
            }
            open_at.to_string_lossy().into_owned()
        };
        let db: Database = Builder::new_local(&path).build().await.map_err(|e| {
            // 报错按因分类，别把环境问题一律报成「数据损坏」引人删库：
            // 锁冲突单列；I/O 族是文件系统层面打不开（数据未必坏）；剩下的才是真损坏。
            let text = e.to_string();
            if text.contains("Locking error") || text.contains("locked by another") {
                AppError::StoreLocked(format!("打开数据库失败: {text}"))
            } else if matches!(&e, turso::Error::IoError(..)) {
                AppError::Io(format!("打开数据库失败: {text}"))
            } else {
                AppError::StoreCorrupt(format!("打开数据库失败: {text}"))
            }
        })?;
        let conn = db
            .connect()
            .map_err(|e| AppError::StoreCorrupt(format!("建立连接失败: {e}")))?;
        // Database 不能 drop（会带走 IO 线程），用 forget 语义保活；
        // Connection 内部持有对实例的引用，进程退出时统一回收。
        std::mem::forget(db);
        let this = Self { conn };
        this.migrate().await?;
        Ok(this)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 把 schema 升到 [`SCHEMA_VERSION`]。重复调用幂等。
    async fn migrate(&self) -> AppResult<()> {
        let version = self.pragma_i64("PRAGMA user_version").await?;
        if version > SCHEMA_VERSION {
            // 未来版本库被旧程序打开：拒绝而不是破坏。
            return Err(AppError::StoreCorrupt(format!(
                "数据库 schema 版本 {version} 高于当前程序支持的 {SCHEMA_VERSION}，请升级应用"
            )));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let target = (i + 1) as i64;
            self.execute("BEGIN", ()).await?;
            match self.run_migration_step(sql, target).await {
                Ok(()) => {
                    self.execute("COMMIT", ()).await?;
                }
                Err(e) => {
                    // 迁移失败回滚，保留旧数据，下次启动可重试。
                    let _ = self.execute("ROLLBACK", ()).await;
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// 单个迁移步：DDL + 版本号写入同一事务（Turso DDL 事务性，失败整体回滚）。`execute` 只跑第一条语句，多语句 DDL 必须走 `execute_batch`（遇错即停，回滚由外层事务负责）；PRAGMA 不接受绑定参数，版本号只能内联拼接——`target` 是自算 i64，不存在注入面。
    async fn run_migration_step(&self, sql: &str, target: i64) -> AppResult<()> {
        self.conn
            .execute_batch(sql)
            .await
            .map_err(|e| AppError::StoreCorrupt(format!("迁移 SQL 执行失败: {e}")))?;
        self.execute(&format!("PRAGMA user_version = {target}"), ())
            .await?;
        Ok(())
    }

    /// 执行无结果集语句，返回受影响行数。
    pub async fn execute<P: turso::IntoParams>(&self, sql: &str, params: P) -> AppResult<u64> {
        self.conn
            .execute(sql, params)
            .await
            .map_err(|e| AppError::StoreCorrupt(format!("SQL 执行失败: {e}")))
    }

    /// 查询单个整型结果（PRAGMA / COUNT 等）。
    async fn pragma_i64(&self, sql: &str) -> AppResult<i64> {
        let mut rows = self
            .conn
            .query(sql, ())
            .await
            .map_err(|e| AppError::StoreCorrupt(format!("PRAGMA 查询失败: {e}")))?;
        let row = rows
            .next()
            .await
            .map_err(|e| AppError::StoreCorrupt(format!("PRAGMA 读取失败: {e}")))?
            .ok_or_else(|| AppError::StoreCorrupt("PRAGMA 无结果".to_string()))?;
        let v = row
            .get_value(0)
            .map_err(|e| AppError::StoreCorrupt(format!("PRAGMA 取值失败: {e}")))?;
        v.as_integer()
            .copied()
            .ok_or_else(|| AppError::StoreCorrupt("PRAGMA 结果不是整型".to_string()))
    }

    /// 事务收口：`f` 返回 Ok 则 COMMIT，Err 则 ROLLBACK。
    /// `f` 拿到的是同一连接上的借用，事务语句不会插错连接。
    pub async fn with_tx<F, Fut, T>(&self, f: F) -> AppResult<T>
    where
        F: FnOnce(Connection) -> Fut,
        Fut: std::future::Future<Output = AppResult<T>>,
    {
        self.execute("BEGIN", ()).await?;
        match f(self.conn.clone()).await {
            Ok(v) => {
                self.execute("COMMIT", ()).await?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.execute("ROLLBACK", ()).await;
                Err(e)
            }
        }
    }
}
