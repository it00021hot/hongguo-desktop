//! merge_tasks 实体的 SQL 读写：整行 JSON，同 id 覆盖。

use turso::Value;

use crate::db::Db;
use hongguo_common::model::MergeTask;
use hongguo_common::{AppError, AppResult};

use super::{col_text, map_db_err};

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
