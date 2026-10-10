//! series 实体的 SQL 读写：整行 JSON，dismissed 权威值提升为列。

use turso::Value;

use crate::domain::model::Series;
use crate::error::{AppError, AppResult};
use crate::store::db::Db;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::entity::{mem_db, rt};

    #[test]
    fn series_upsert_preserves_dismissed_and_returns_by_id() {
        let db = mem_db();
        rt().block_on(async {
            // dismissed 的写入入口已随「移除记录」功能下线，这里直接
            // 构造一条带旧标记的档案，验证 upsert 不复活它
            let s = Series {
                series_id: "1".into(),
                title: "剧".into(),
                dismissed: true,
                ..Default::default()
            };
            upsert_series(&db, &s).await.unwrap();

            // 重新解析登记不复活
            let fresh = Series {
                dismissed: false,
                title: "剧（新版）".into(),
                ..s.clone()
            };
            upsert_series(&db, &fresh).await.unwrap();
            let back = series_by_id(&db, "1").await.unwrap().expect("应当还在");
            assert!(back.dismissed, "移除标记必须保留");
            assert_eq!(back.title, "剧（新版）");
        });
    }
}
