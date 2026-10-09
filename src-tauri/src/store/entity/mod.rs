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

use turso::Value;

use crate::error::{AppError, AppResult};
use crate::store::db::Db;

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

mod device;
mod merge;
mod playback;
mod series;
mod settings;
mod task;

pub use device::{
    backup_device_profile, device_meta_get, device_meta_set, device_profile, device_profile_backup,
    save_device_profile,
};
pub use merge::{delete_merge_task, merge_tasks, upsert_merge_task};
pub use playback::{playback_position, save_playback_position, series_last_position};
pub use series::{series_all, series_by_id, upsert_series};
pub use settings::{save_settings, settings};
pub use task::{replace_tasks, tasks};

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
        task::insert_tasks(conn, &legacy.tasks).await?;
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

// ---------- 测试公共件（各实体子模块测试共用） ----------

#[cfg(test)]
fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("测试 runtime")
}

#[cfg(test)]
fn mem_db() -> Db {
    rt().block_on(Db::open(":memory:")).expect("内存库")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::model::settings::Settings;
    use crate::domain::model::{
        DownloadTask, MergeMode, MergeTask, PlaybackPosition, Series, TaskStatus,
    };

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
                [(1u32, PlaybackPosition::new(5.0, 90.0))]
                    .into_iter()
                    .collect(),
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
