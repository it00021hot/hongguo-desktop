//! 实体读写：settings / tasks / series / playback / merge_tasks 的 SQL 实现。
//!
//! 设计约定：
//! - 复杂实体（任务/剧集/合并）整行存 JSON 列，serde 模型与历史 alias
//!   兼容逻辑原样复用；只把需要 WHERE / ORDER / JOIN 的字段提升为列。
//! - playback 是最热的写入点（前端 5 秒节流上报），全真实列 + UPSERT。
//! - 所有多行写操作包在单个事务里，中途失败不落半截数据。
//!
//! 本模块全部是 `async fn`，跑在 [`crate::store::bridge`] 的专用 DB 线程上，
//! 由同步门面 [`crate::store::Store`] 逐方法转调；不要在业务代码里直接调用。

use turso::{Connection, Value};

use crate::domain::model::settings::Settings;
use crate::domain::model::{DownloadTask, MergeTask, PlaybackPosition, Series};
use crate::error::{AppError, AppResult};
use crate::store::db::Db;

/// playback 历史查询的一行：某部剧最近播放的那一集。
#[derive(Debug, Clone, PartialEq)]
pub struct LatestPlayback {
    pub series_id: String,
    pub vid_index: u32,
    pub position: PlaybackPosition,
}

// ---------- 取列小工具 ----------

fn col_text(row: &turso::Row, i: usize) -> AppResult<String> {
    row.get_value(i)
        .ok()
        .and_then(|v| v.as_text().cloned())
        .ok_or_else(|| AppError::StoreCorrupt(format!("第 {i} 列不是文本")))
}

fn col_i64(row: &turso::Row, i: usize) -> AppResult<i64> {
    row.get_value(i)
        .ok()
        .and_then(|v| v.as_integer().copied())
        .ok_or_else(|| AppError::StoreCorrupt(format!("第 {i} 列不是整型")))
}

fn col_f64(row: &turso::Row, i: usize) -> AppResult<f64> {
    row.get_value(i)
        .ok()
        .and_then(|v| v.as_real().copied())
        .ok_or_else(|| AppError::StoreCorrupt(format!("第 {i} 列不是浮点")))
}

/// 枚举状态序列化成落库字符串（"pending" / "running" / …）。
fn status_text<T: serde::Serialize>(status: &T) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn map_db_err(e: turso::Error) -> AppError {
    AppError::StoreCorrupt(format!("SQL 失败: {e}"))
}

// ---------- settings ----------

/// 读设置。没有落过盘时返回 None（调用方用默认值）。
pub async fn settings(db: &Db) -> AppResult<Option<Settings>> {
    let mut rows = db.conn().query("SELECT json FROM settings WHERE id = 1", ()).await.map_err(map_db_err)?;
    let Some(row) = rows.next().await.map_err(map_db_err)? else {
        return Ok(None);
    };
    let json = col_text(&row, 0)?;
    serde_json::from_str(&json)
        .map(Some)
        .map_err(|e| AppError::StoreCorrupt(format!("设置行反序列化失败: {e}")))
}

/// 写设置（单行表）。
pub async fn save_settings(db: &Db, s: &Settings) -> AppResult<()> {
    let json = serde_json::to_string(s)
        .map_err(|e| AppError::StoreCorrupt(format!("设置序列化失败: {e}")))?;
    db.execute(
        "INSERT INTO settings (id, json) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET json = excluded.json",
        [Value::Text(json)],
    )
    .await?;
    Ok(())
}

// ---------- tasks ----------

/// 全部下载任务，按登记顺序（rowid 即插入序，等价于旧的 Vec 顺序）。
pub async fn tasks(db: &Db) -> AppResult<Vec<DownloadTask>> {
    let mut rows = db
        .conn()
        .query("SELECT json FROM tasks ORDER BY rowid", ())
        .await
        .map_err(map_db_err)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_db_err)? {
        let json = col_text(&row, 0)?;
        let t: DownloadTask = serde_json::from_str(&json)
            .map_err(|e| AppError::StoreCorrupt(format!("任务行反序列化失败: {e}")))?;
        out.push(t);
    }
    Ok(out)
}

/// 用队列快照整体替换任务表（persist_tasks 的 DB 形态）。
///
/// DELETE + 批量 INSERT 放在一个事务里：这是一份内存快照的镜像，
/// 任何时刻中断都不能留下「半新半旧」的混合。
pub async fn replace_tasks(db: &Db, snapshot: &[DownloadTask]) -> AppResult<()> {
    db.with_tx(|conn| async move {
        let conn = &conn;
        conn.execute("DELETE FROM tasks", ()).await.map_err(map_db_err)?;
        insert_tasks(conn, snapshot).await?;
        Ok(())
    })
    .await
}

async fn insert_tasks(conn: &Connection, tasks: &[DownloadTask]) -> AppResult<()> {
    for t in tasks {
        let json = serde_json::to_string(t)
            .map_err(|e| AppError::StoreCorrupt(format!("任务序列化失败: {e}")))?;
        conn.execute(
            "INSERT OR REPLACE INTO tasks (id, series_id, status, json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            [
                Value::Text(t.id.clone()),
                Value::Text(t.series_id.clone()),
                Value::Text(status_text(&t.status)),
                Value::Text(json),
                Value::Integer(t.updated_at),
            ],
        )
        .await
        .map_err(map_db_err)?;
    }
    Ok(())
}

// ---------- series ----------

/// 全部剧集档案（含 dismissed），按登记顺序。
pub async fn series_all(db: &Db) -> AppResult<Vec<Series>> {
    let mut rows = db
        .conn()
        .query("SELECT json, dismissed FROM series ORDER BY rowid", ())
        .await
        .map_err(map_db_err)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_db_err)? {
        let json = col_text(&row, 0)?;
        let mut s: Series = serde_json::from_str(&json)
            .map_err(|e| AppError::StoreCorrupt(format!("剧行反序列化失败: {e}")))?;
        // dismissed 的权威值在列上：json 里的是登记时的快照，会被
        // set_dismissed / upsert 的保留语义甩在后面
        s.dismissed = col_i64(&row, 1)? != 0;
        out.push(s);
    }
    Ok(out)
}

/// 按 id 取档案（含 dismissed 的也取得到）。
pub async fn series_by_id(db: &Db, series_id: &str) -> AppResult<Option<Series>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT json, dismissed FROM series WHERE series_id = ?1",
            [Value::Text(series_id.to_string())],
        )
        .await
        .map_err(map_db_err)?;
    let Some(row) = rows.next().await.map_err(map_db_err)? else {
        return Ok(None);
    };
    let json = col_text(&row, 0)?;
    let mut s: Series = serde_json::from_str(&json)
        .map_err(|e| AppError::StoreCorrupt(format!("剧行反序列化失败: {e}")))?;
    s.dismissed = col_i64(&row, 1)? != 0;
    Ok(Some(s))
}

/// 登记或更新档案。冲突时**保留 dismissed 标记**（重新拉取不能复活已移除的剧）。
pub async fn upsert_series(db: &Db, s: &Series) -> AppResult<()> {
    let json = serde_json::to_string(s)
        .map_err(|e| AppError::StoreCorrupt(format!("剧序列化失败: {e}")))?;
    let now = chrono::Utc::now().timestamp_millis();
    db.execute(
        "INSERT INTO series (series_id, dismissed, json, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(series_id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
        [
            Value::Text(s.series_id.clone()),
            Value::Integer(i64::from(s.dismissed)),
            Value::Text(json),
            Value::Integer(now),
        ],
    )
    .await?;
    Ok(())
}

/// 标记移除。返回实际改变的行数（0 = 没这部剧，调用方报 NotFound）。
pub async fn set_dismissed(db: &Db, series_id: &str) -> AppResult<u64> {
    db.execute(
        "UPDATE series SET dismissed = 1 WHERE series_id = ?1 AND dismissed = 0",
        [Value::Text(series_id.to_string())],
    )
    .await
}

/// 一次性移除全部可见档案，返回移除条数。
pub async fn dismiss_all(db: &Db) -> AppResult<u64> {
    db.execute("UPDATE series SET dismissed = 1 WHERE dismissed = 0", ())
        .await
}

// ---------- playback ----------

/// 读单集位置。
pub async fn playback_position(
    db: &Db,
    series_id: &str,
    vid_index: u32,
) -> AppResult<Option<PlaybackPosition>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT \"current_time\", duration, updated_at FROM playback
             WHERE series_id = ?1 AND vid_index = ?2",
            [Value::Text(series_id.to_string()), Value::Integer(vid_index as i64)],
        )
        .await
        .map_err(map_db_err)?;
    let Some(row) = rows.next().await.map_err(map_db_err)? else {
        return Ok(None);
    };
    Ok(Some(PlaybackPosition {
        current_time: col_f64(&row, 0)?,
        duration: col_f64(&row, 1)?,
        updated_at: col_i64(&row, 2)?,
    }))
}

/// 写单集位置（最热写路径：前端 5 秒一次，单行 UPSERT）。
pub async fn save_playback_position(
    db: &Db,
    series_id: &str,
    vid_index: u32,
    pos: &PlaybackPosition,
) -> AppResult<()> {
    db.execute(
        "INSERT INTO playback (series_id, vid_index, \"current_time\", duration, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(series_id, vid_index) DO UPDATE SET
            \"current_time\" = excluded.\"current_time\",
            duration = excluded.duration,
            updated_at = excluded.updated_at",
        [
            Value::Text(series_id.to_string()),
            Value::Integer(vid_index as i64),
            Value::Real(pos.current_time),
            Value::Real(pos.duration),
            Value::Integer(pos.updated_at),
        ],
    )
    .await?;
    Ok(())
}

/// 删一部剧的全部进度，返回删除行数（0 = 本来就没有）。
pub async fn remove_playback(db: &Db, series_id: &str) -> AppResult<u64> {
    db.execute(
        "DELETE FROM playback WHERE series_id = ?1",
        [Value::Text(series_id.to_string())],
    )
    .await
}

/// 清空全部进度，返回删除行数。
pub async fn clear_playback(db: &Db) -> AppResult<u64> {
    db.execute("DELETE FROM playback", ()).await
}

/// 每部剧最近播放的那一集，按时间倒序。
///
/// 同一部剧同一毫秒写了两集的话 JOIN 会出两行，在 Rust 侧按 series_id
/// 去重保留第一条（ORDER BY 已保证是最新的）。
pub async fn playback_latest_per_series(db: &Db) -> AppResult<Vec<LatestPlayback>> {
    let mut rows = db
        .conn()
        .query(
            r#"
            SELECT p.series_id, p.vid_index, p.current_time, p.duration, p.updated_at
            FROM playback p
            JOIN (SELECT series_id, MAX(updated_at) AS max_updated
                  FROM playback GROUP BY series_id) m
              ON m.series_id = p.series_id AND m.max_updated = p.updated_at
            ORDER BY p.updated_at DESC
            "#,
            (),
        )
        .await
        .map_err(map_db_err)?;
    let mut out: Vec<LatestPlayback> = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_db_err)? {
        let series_id = col_text(&row, 0)?;
        if out.iter().any(|l| l.series_id == series_id) {
            continue;
        }
        out.push(LatestPlayback {
            series_id,
            vid_index: u32::try_from(col_i64(&row, 1)?)
                .map_err(|e| AppError::StoreCorrupt(format!("集号越界: {e}")))?,
            position: PlaybackPosition {
                current_time: col_f64(&row, 2)?,
                duration: col_f64(&row, 3)?,
                updated_at: col_i64(&row, 4)?,
            },
        });
    }
    Ok(out)
}

// ---------- merge_tasks ----------

/// 全部合并任务，按登记顺序。
pub async fn merge_tasks(db: &Db) -> AppResult<Vec<MergeTask>> {
    let mut rows = db
        .conn()
        .query("SELECT json FROM merge_tasks ORDER BY rowid", ())
        .await
        .map_err(map_db_err)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_db_err)? {
        let json = col_text(&row, 0)?;
        let t: MergeTask = serde_json::from_str(&json)
            .map_err(|e| AppError::StoreCorrupt(format!("合并行反序列化失败: {e}")))?;
        out.push(t);
    }
    Ok(out)
}

/// 写一条合并任务（开始时插 running、结束时覆盖，都是它）。
pub async fn upsert_merge_task(db: &Db, t: &MergeTask) -> AppResult<()> {
    let json = serde_json::to_string(t)
        .map_err(|e| AppError::StoreCorrupt(format!("合并任务序列化失败: {e}")))?;
    db.execute(
        "INSERT INTO merge_tasks (id, json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
        [
            Value::Text(t.id.clone()),
            Value::Text(json),
            Value::Integer(chrono::Utc::now().timestamp_millis()),
        ],
    )
    .await?;
    Ok(())
}

/// 删一条合并任务记录，返回删除行数（0 = 不存在）。
pub async fn delete_merge_task(db: &Db, id: &str) -> AppResult<u64> {
    db.execute(
        "DELETE FROM merge_tasks WHERE id = ?1",
        [Value::Text(id.to_string())],
    )
    .await
}

// ---------- 旧档导入 ----------

/// 把解析好的旧 data.json 内容一次性灌库（幂等：全部 INSERT OR REPLACE）。
///
/// 幂等是刻意的：迁移完成后如果改名 `data.json.migrated` 失败，
/// 下次启动会再走一遍导入，绝不能造出重复行。
pub async fn import_legacy(db: &Db, legacy: &super::json_migrate::LegacyData) -> AppResult<()> {
    db.with_tx(|conn| async move {
        let conn = &conn;
        if let Some(s) = &legacy.settings {
            let json = serde_json::to_string(s)
                .map_err(|e| AppError::StoreCorrupt(format!("设置序列化失败: {e}")))?;
            conn.execute(
                "INSERT OR REPLACE INTO settings (id, json) VALUES (1, ?1)",
                [Value::Text(json)],
            )
            .await
            .map_err(map_db_err)?;
        }
        insert_tasks(conn, &legacy.tasks).await?;
        for s in &legacy.series {
            let json = serde_json::to_string(s)
                .map_err(|e| AppError::StoreCorrupt(format!("剧序列化失败: {e}")))?;
            conn.execute(
                "INSERT OR REPLACE INTO series (series_id, dismissed, json, updated_at)
                 VALUES (?1, ?2, ?3, ?4)",
                [
                    Value::Text(s.series_id.clone()),
                    Value::Integer(i64::from(s.dismissed)),
                    Value::Text(json),
                    Value::Integer(chrono::Utc::now().timestamp_millis()),
                ],
            )
            .await
            .map_err(map_db_err)?;
        }
        for (series_id, episodes) in &legacy.playback {
            for (vid_index, pos) in episodes {
                conn.execute(
                    "INSERT OR REPLACE INTO playback
                        (series_id, vid_index, \"current_time\", duration, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    [
                        Value::Text(series_id.clone()),
                        Value::Integer(i64::from(*vid_index)),
                        Value::Real(pos.current_time),
                        Value::Real(pos.duration),
                        Value::Integer(pos.updated_at),
                    ],
                )
                .await
                .map_err(map_db_err)?;
            }
        }
        for t in &legacy.merge_tasks {
            let json = serde_json::to_string(t)
                .map_err(|e| AppError::StoreCorrupt(format!("合并任务序列化失败: {e}")))?;
            conn.execute(
                "INSERT OR REPLACE INTO merge_tasks (id, json, updated_at) VALUES (?1, ?2, ?3)",
                [
                    Value::Text(t.id.clone()),
                    Value::Text(json),
                    Value::Integer(chrono::Utc::now().timestamp_millis()),
                ],
            )
            .await
            .map_err(map_db_err)?;
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{MergeMode, MergeStatus, TaskStatus};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("测试 runtime")
    }

    fn mem_db() -> Db {
        rt().block_on(Db::open(":memory:")).expect("内存库")
    }

    #[test]
    fn settings_roundtrip_and_missing_returns_none() {
        let db = mem_db();
        rt().block_on(async {
            assert!(settings(&db).await.unwrap().is_none(), "没写过就该是 None");
            save_settings(&db, &Settings::default()).await.unwrap();
            assert!(settings(&db).await.unwrap().is_some());
        });
    }

    #[test]
    fn replace_tasks_is_a_full_mirror() {
        let db = mem_db();
        rt().block_on(async {
            let a = DownloadTask::new("1", "剧", 1, "v1", "第一集");
            let b = DownloadTask::new("1", "剧", 2, "v2", "第二集");
            replace_tasks(&db, &[a.clone(), b.clone()]).await.unwrap();
            assert_eq!(tasks(&db).await.unwrap().len(), 2);

            // 快照里删掉一条，镜像后任务表也要少一条
            replace_tasks(&db, std::slice::from_ref(&a)).await.unwrap();
            let list = tasks(&db).await.unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].id, a.id);
        });
    }

    #[test]
    fn series_upsert_preserves_dismissed_and_returns_by_id() {
        let db = mem_db();
        rt().block_on(async {
            let s = Series {
                series_id: "1".into(),
                title: "剧".into(),
                ..Default::default()
            };
            upsert_series(&db, &s).await.unwrap();
            assert_eq!(set_dismissed(&db, "1").await.unwrap(), 1);

            // 重新解析登记不复活
            let fresh = Series {
                title: "剧（新版）".into(),
                ..s.clone()
            };
            upsert_series(&db, &fresh).await.unwrap();
            let back = series_by_id(&db, "1").await.unwrap().expect("应当还在");
            assert!(back.dismissed, "移除标记必须保留");
            assert_eq!(back.title, "剧（新版）");

            // 重复 dismiss 返回 0 行（NotFound 判据）
            assert_eq!(set_dismissed(&db, "1").await.unwrap(), 0);
        });
    }

    #[test]
    fn playback_upsert_is_per_episode() {
        let db = mem_db();
        rt().block_on(async {
            // updated_at 显式给定：PlaybackPosition::new 打的是毫秒时间戳，
            // 三条同毫秒构造的话「最新一集」断言会因同值排序不定而随机失败
            let at = |t: f64, updated: i64| PlaybackPosition {
                current_time: t,
                duration: 100.0,
                updated_at: updated,
            };
            let p1 = at(10.0, 100);
            save_playback_position(&db, "A", 1, &p1).await.unwrap();
            let p2 = at(50.0, 300);
            save_playback_position(&db, "A", 2, &p2).await.unwrap();
            // 同一集再写是覆盖不是新增
            let p1b = at(20.0, 200);
            save_playback_position(&db, "A", 1, &p1b).await.unwrap();

            assert_eq!(
                playback_position(&db, "A", 1).await.unwrap().map(|p| p.current_time),
                Some(20.0)
            );

            let latest = playback_latest_per_series(&db).await.unwrap();
            assert_eq!(latest.len(), 1, "一部剧只出一条");
            assert_eq!(latest[0].vid_index, 2, "取最近更新的一集");
            assert_eq!(latest[0].series_id, "A");

            assert_eq!(remove_playback(&db, "A").await.unwrap(), 2, "整部剧两行都删");
            assert_eq!(remove_playback(&db, "A").await.unwrap(), 0);
        });
    }

    #[test]
    fn merge_task_upsert_delete() {
        let db = mem_db();
        rt().block_on(async {
            let mut t = MergeTask::new("1", "剧", "out", MergeMode::Quick);
            t.status = MergeStatus::Running;
            upsert_merge_task(&db, &t).await.unwrap();
            t.status = MergeStatus::Completed;
            upsert_merge_task(&db, &t).await.unwrap();

            let list = merge_tasks(&db).await.unwrap();
            assert_eq!(list.len(), 1, "同 id 覆盖不新增");
            assert_eq!(list[0].status, MergeStatus::Completed);

            assert_eq!(delete_merge_task(&db, &t.id).await.unwrap(), 1);
            assert_eq!(delete_merge_task(&db, &t.id).await.unwrap(), 0);
            assert!(merge_tasks(&db).await.unwrap().is_empty());
        });
    }

    #[test]
    fn legacy_import_is_idempotent() {
        let db = mem_db();
        let legacy = super::super::json_migrate::LegacyData {
            settings: Some(Settings {
                max_concurrency: 8,
                ..Settings::default()
            }),
            tasks: vec![DownloadTask::new("1", "剧", 1, "v1", "第一集")],
            series: vec![Series {
                series_id: "1".into(),
                title: "剧".into(),
                ..Default::default()
            }],
            playback: [(
                "A".to_string(),
                [(1u32, PlaybackPosition::new(5.0, 90.0))].into_iter().collect(),
            )]
            .into_iter()
            .collect(),
            merge_tasks: vec![MergeTask::new("1", "剧", "o", MergeMode::Quick)],
        };
        rt().block_on(async {
            super::import_legacy(&db, &legacy).await.unwrap();
            // 同一份导两遍：幂等，不翻倍
            super::import_legacy(&db, &legacy).await.unwrap();

            assert_eq!(tasks(&db).await.unwrap().len(), 1);
            assert_eq!(series_all(&db).await.unwrap().len(), 1);
            assert_eq!(merge_tasks(&db).await.unwrap().len(), 1);
            assert_eq!(settings(&db).await.unwrap().unwrap().max_concurrency, 8);
            assert!(playback_position(&db, "A", 1).await.unwrap().is_some());
        });
    }

    #[test]
    fn task_status_column_matches_model() {
        // status 列是给将来 WHERE 用的，必须与 serde 形态一致（小写驼峰、无引号）
        assert_eq!(status_text(&TaskStatus::Running), "running");
    }
}
