//! series 实体的 SQL 读写：整行 JSON，dismissed 权威值提升为列。

use turso::Value;

use crate::db::Db;
use hongguo_common::model::Series;
use hongguo_common::{AppError, AppResult};

use super::{col_i64, col_text, map_db_err};

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
        // dismissed 的权威值在列上：json 里的是登记时的快照，会被 upsert 的保留语义甩在后面
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

/// Fetch only the requested series in one indexed query.
pub async fn series_by_ids(db: &Db, ids: &[String]) -> AppResult<Vec<Series>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    let sql = format!("SELECT json, dismissed FROM series WHERE series_id IN ({placeholders})");
    let params: Vec<Value> = ids.iter().cloned().map(Value::Text).collect();
    let mut rows = db.conn().query(&sql, params).await.map_err(map_db_err)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_db_err)? {
        let mut series: Series = serde_json::from_str(&col_text(&row, 0)?)
            .map_err(|error| AppError::StoreCorrupt(format!("剧行反序列化失败: {error}")))?;
        series.dismissed = col_i64(&row, 1)? != 0;
        out.push(series);
    }
    Ok(out)
}

/// 登记或更新档案。冲突时**保留 dismissed 标记**（历史库里的移除标记
/// 不能被重新拉取复活；新代码已没有写入入口，列只为兼容旧库保留）。
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
