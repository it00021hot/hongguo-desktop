//! settings 实体的 SQL 读写：单行 JSON 表。

use turso::Value;

use crate::domain::model::settings::Settings;
use crate::error::{AppError, AppResult};
use crate::store::db::Db;

use super::{col_text, map_db_err};

/// 读设置。没有落过盘时返回 None（调用方用默认值）。
pub async fn settings(db: &Db) -> AppResult<Option<Settings>> {
    let mut rows = db
        .conn()
        .query("SELECT json FROM settings WHERE id = 1", ())
        .await
        .map_err(map_db_err)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::entity::{mem_db, rt};

    #[test]
    fn settings_roundtrip_and_missing_returns_none() {
        let db = mem_db();
        rt().block_on(async {
            assert!(settings(&db).await.unwrap().is_none(), "没写过就该是 None");
            save_settings(&db, &Settings::default()).await.unwrap();
            assert!(settings(&db).await.unwrap().is_some());
        });
    }
}
