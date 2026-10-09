//! 内嵌 Turso 数据库：连接与 schema 迁移。
//!
//! 选型背景（M1，2026-10）：替换原 data.json 单文件全量重写存储。
//! `turso` crate 是 tursodatabase/turso 的官方 Rust API，纯 Rust、无 C 工具链，
//! 本地内嵌形态（`Builder::new_local`）。刻意关闭 default features：
//! mimalloc 是全局分配器不能让库替我们换；fts 暂时用不上。
//!
//! 并发模型：`Connection` 内部走连接级串行（Turso 引擎语义），
//! 多个 `connect()` 共享同一数据库文件由库自身做页级加锁，
//! 因此 AppState 里不再需要 `RwLock<DataStore>`，仓储层拿 `Db` 的克隆即可。
//!
//! 事务边界：`begin/commit/rollback` 是三个独立 SQL 语句，不是持有式 API，
//! 调用方必须在同一 async 任务里成对使用；仓储层提供 `with_tx` 收口。

use std::path::Path;

use turso::{Builder, Connection, Database};

use crate::error::{AppError, AppResult};

/// 当前 schema 版本。每次结构性变更（加表/加列）时 +1，
/// 并在 [`MIGRATIONS`] 追加一段从上一版本到新版本的 SQL。
const SCHEMA_VERSION: i64 = 5;

/// 按版本升序排列的迁移 SQL，第 n 段把 schema 从版本 n 升到 n+1。
pub static MIGRATIONS: &[&str] = &[
    // v0 -> v1：初始五张表，对应原 DataStore 的五个字段。
    // 设计约定：
    // - 复杂实体（任务/剧集/合并）整行存 JSON 列，serde 模型与 26 处
    //   alias 兼容逻辑原样复用；只把需要 WHERE/ORDER 的字段提升为列。
    // - playback 是最热的写入点（前端 5 秒节流上报），全真实列 +
    //   UPSERT，彻底消灭整文件重写。
    // - settings 单行表（id=1），与原单例语义一致。
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
    // v1 -> v2：账号时代的基础表。
    // - device_profile 单行表：设备注册（M2b）产出的档案整份 JSON 落这里，
    //   启动装载，签名链路统一从 AppState 取。
    // - session 单行表：登录态（M3）的 Cookie 与用户资料。先建表占位，
    //   访问器随登录模块一起落。
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
    // v2 -> v3：webp 封面缓存。信息流的 HEIC 封面 WebView2 渲染不了，
    // 官网详情页有 webp 版；按 series_id 缓存，一部剧只抓一次。
    r#"
    CREATE TABLE cover_cache (
        series_id  TEXT PRIMARY KEY,
        cover      TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    "#,
    // v3 -> v4：官网链路整体下线（数据一律走官方 App 接口），
    // webp 封面缓存随之作废——HEIC 封面改由本地 hongguo-cover 协议现转。
    r#"
    DROP TABLE IF EXISTS cover_cache;
    "#,
    // v4 -> v5：设备档案从单行表扩成多行（1=现役，2=上一代备份，10=
    // bootstrap 元数据）。老表的 CHECK (id = 1) 锁死单行，SQLite 不能
    // 直接删约束，按惯例重建表搬数据（对齐 hgplayer device.json + .bak
    // 的「现役 + 上一代」双档案形态）。
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
];

/// 数据库句柄。`Database` 持有 IO 线程必须存活，`Connection` 可克隆共享。
#[derive(Clone)]
pub struct Db {
    conn: Connection,
}

impl Db {
    /// 打开（或创建）数据库文件并跑到最新 schema。
    ///
    /// 传入 `":memory:"` 得到内存库，测试用。
    pub async fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref().to_string_lossy().into_owned();
        // 全新安装时数据目录整条不存在，而 Turso 的 build() 只建文件不建
        // 父目录，直接报 I/O entity not found——曾被映射成「数据损坏」
        // 引人删库，v0.1.0 全新装机因此必闪退。这里是所有文件库开库的
        // 唯一收口，建目录收进来，其余落盘方（缓存/下载/诊断）本就各自
        // create_dir_all。
        if path != ":memory:" {
            if let Some(dir) = Path::new(&path).parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| AppError::Io(format!("创建数据目录失败: {e}")))?;
            }
        }
        let db: Database = Builder::new_local(&path).build().await.map_err(|e| {
            // 报错按因分类，别把环境问题一律报成「数据损坏」引人删库。
            // 锁冲突单列（另一个实例还着库）；I/O 族（NotFound /
            // PermissionDenied 等）是文件系统层面打不开，数据本身未必坏；
            // 剩下的（Corrupt/NotAdb）才是真损坏。
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

    /// 单个迁移步：DDL + 版本号写入同一事务。
    /// Turso 的 DDL 是事务性的，失败即整体回滚。
    ///
    /// 两个实现细节：
    /// - `execute` 只跑**第一条**语句（Turso 的既定语义），多语句 DDL 必须走
    ///   `execute_batch`（顺序执行、遇错即停，但不回滚——回滚由外层事务负责）。
    /// - PRAGMA 不接受绑定参数，版本号只能内联拼接；
    ///   `target` 是我们自己在 [`MIGRATIONS`] 索引上算出的 i64，不存在注入面。
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

#[cfg(test)]
mod tests {
    use super::*;
    use turso::Value;

    /// 异步测试的迷你运行时：项目 test 依赖里没有 tokio 的 macro 也够不着 dev-deps，
    /// 直接用 turso 自带的 block_on 语义不可靠，这里手写一个最简 future 泵。
    /// （如果后续 tokio::test 可用，应优先换掉。）
    fn run<F: std::future::Future>(fut: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("构建测试 runtime")
            .block_on(fut)
    }

    fn tmp_db_path(tag: &str) -> String {
        let dir = std::env::temp_dir().join(format!("hongguo-spike-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("test.db").to_string_lossy().into_owned()
    }

    #[test]
    fn spike_basic_crud_upsert_and_json_column() {
        run(async {
            let db = Db::open(":memory:").await.expect("打开内存库");

            // 建表 + 异构参数绑定（TEXT/INTEGER/REAL）+ JSON 列
            db.execute(
                "CREATE TABLE t (k TEXT PRIMARY KEY, n INTEGER, x REAL, j TEXT)",
                (),
            )
            .await
            .expect("建表");
            let n = db
                .execute(
                    "INSERT INTO t (k, n, x, j) VALUES (?1, ?2, ?3, ?4)",
                    [
                        Value::Text("a".into()),
                        Value::Integer(42),
                        Value::Real(1.5),
                        Value::Text(r#"{"播放": 12.5}"#.into()),
                    ],
                )
                .await
                .expect("插入");
            assert_eq!(n, 1);

            // UPSERT：playback 热点的目标形态。刻意只更新部分列，
            // 验证未被 SET 的列保留旧值（部分更新语义）。
            db.execute(
                "INSERT INTO t (k, n, x, j) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(k) DO UPDATE SET n = excluded.n, x = excluded.x",
                [
                    Value::Text("a".into()),
                    Value::Integer(43),
                    Value::Real(2.5),
                    Value::Null,
                ],
            )
            .await
            .expect("UPSERT");

            // 查询读回
            let mut rows = db
                .conn()
                .query(
                    "SELECT n, x, j FROM t WHERE k = ?1",
                    [Value::Text("a".into())],
                )
                .await
                .expect("查询");
            let row = rows.next().await.expect("next 不报错").expect("有一行");
            let n = row.get_value(0).expect("取 n");
            assert_eq!(n.as_integer().copied(), Some(43));
            let x = row.get_value(1).expect("取 x");
            assert_eq!(x.as_real().copied(), Some(2.5));
            // j 未参与 UPSERT 的 SET，应保留首次插入的文本
            let j = row.get_value(2).expect("取 j");
            assert_eq!(j.as_text().map(|s| s.as_str()), Some("{\"播放\": 12.5}"));

            // 再来一次全列 UPSERT，把 j 显式置 NULL
            db.execute(
                "INSERT INTO t (k, n, x, j) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(k) DO UPDATE SET j = excluded.j",
                [
                    Value::Text("a".into()),
                    Value::Integer(43),
                    Value::Real(2.5),
                    Value::Null,
                ],
            )
            .await
            .expect("UPSERT 置 NULL");
            let mut rows = db
                .conn()
                .query("SELECT j FROM t WHERE k = ?1", [Value::Text("a".into())])
                .await
                .expect("查询 2");
            let row = rows.next().await.expect("next 不报错").expect("有一行");
            assert!(
                row.get_value(0).expect("取 j").is_null(),
                "显式 excluded.j = NULL 应当写入 NULL"
            );
        });
    }

    #[test]
    fn spike_transaction_rollback() {
        run(async {
            let db = Db::open(":memory:").await.expect("打开内存库");
            db.execute("CREATE TABLE t (k TEXT PRIMARY KEY)", ())
                .await
                .expect("建表");

            let err = db
                .with_tx(|conn| async move {
                    let _ = conn
                        .execute("INSERT INTO t (k) VALUES ('x')", ())
                        .await
                        .map_err(|e| AppError::StoreCorrupt(e.to_string()))?;
                    // 制造失败：主键冲突
                    conn.execute("INSERT INTO t (k) VALUES ('x')", ())
                        .await
                        .map_err(|e| AppError::StoreCorrupt(e.to_string()))
                })
                .await;
            assert!(err.is_err(), "第二次插入同主键必须失败");

            // 回滚后表必须是空的
            let mut rows = db
                .conn()
                .query("SELECT COUNT(*) FROM t", ())
                .await
                .expect("count");
            let row = rows.next().await.unwrap().unwrap();
            let c = row.get_value(0).unwrap();
            assert_eq!(c.as_integer().copied(), Some(0), "事务回滚后不应有残留行");
        });
    }

    #[test]
    fn spike_file_db_persists_and_migrate_idempotent() {
        run(async {
            let path = tmp_db_path("file");
            let _ = std::fs::remove_file(&path);

            {
                let db = Db::open(&path).await.expect("打开文件库");
                db.execute(
                    "INSERT INTO settings (id, json) VALUES (1, '{\"a\":1}')",
                    (),
                )
                .await
                .expect("写入 settings");
            }
            {
                // 重开：migrate 幂等，数据还在
                let db = Db::open(&path).await.expect("重开文件库");
                let mut rows = db
                    .conn()
                    .query("SELECT json FROM settings WHERE id = 1", ())
                    .await
                    .expect("查询");
                let row = rows.next().await.unwrap().unwrap();
                let j = row.get_value(0).unwrap();
                assert_eq!(j.as_text().map(|s| s.as_str()), Some("{\"a\":1}"));
            }
            let _ = std::fs::remove_file(&path);
        });
    }

    #[test]
    fn spike_multiple_connections_share_file() {
        run(async {
            let path = tmp_db_path("multi");
            let _ = std::fs::remove_file(&path);
            let db = Db::open(&path).await.expect("打开");
            // 同一 Database 的第二个连接：仓储层并发用的形态
            let other = db.clone();
            db.execute(
                "INSERT INTO series (series_id, dismissed, json, updated_at) VALUES ('s1', 0, '{}', 1)",
                (),
            )
            .await
            .expect("连接 1 写入");
            let mut rows = other
                .conn()
                .query("SELECT COUNT(*) FROM series", ())
                .await
                .expect("连接 2 查询");
            let row = rows.next().await.unwrap().unwrap();
            assert_eq!(row.get_value(0).unwrap().as_integer().copied(), Some(1));
            let _ = std::fs::remove_file(&path);
        });
    }
}
