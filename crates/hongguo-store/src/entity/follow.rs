//! follow_baseline / follow_notice 的 SQL 读写（追更检查的本地两件套）。

use turso::Value;

use crate::db::Db;
use hongguo_common::model::FollowNotice;
use hongguo_common::{AppError, AppResult};

use super::{col_i64, col_text, map_db_err};

/// 某剧的基线集数；无基线（首次见到）返回 `None`。
pub async fn baseline_get(db: &Db, series_id: &str) -> AppResult<Option<i64>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT episode_cnt FROM follow_baseline WHERE series_id = ?1",
            [Value::Text(series_id.to_string())],
        )
        .await
        .map_err(map_db_err)?;
    match rows.next().await.map_err(map_db_err)? {
        Some(row) => Ok(Some(col_i64(&row, 0)?)),
        None => Ok(None),
    }
}

/// 记/更新基线。
pub async fn baseline_set(db: &Db, series_id: &str, episode_cnt: i64) -> AppResult<()> {
    db.execute(
        "INSERT INTO follow_baseline (series_id, episode_cnt, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(series_id) DO UPDATE SET episode_cnt = excluded.episode_cnt, updated_at = excluded.updated_at",
        [
            Value::Text(series_id.to_string()),
            Value::Integer(episode_cnt),
            Value::Integer(chrono::Utc::now().timestamp()),
        ],
    )
    .await?;
    Ok(())
}

/// 删基线（取消追更时调：重追从当前集数重新起算，不补发历史更新）。
pub async fn baseline_delete(db: &Db, series_id: &str) -> AppResult<()> {
    db.execute(
        "DELETE FROM follow_baseline WHERE series_id = ?1",
        [Value::Text(series_id.to_string())],
    )
    .await?;
    Ok(())
}

/// 落一条通知，返回行 id。
pub async fn notice_insert(
    db: &Db,
    kind: &str,
    series_id: &str,
    title: &str,
    detail: &str,
    at: i64,
) -> AppResult<i64> {
    db.execute(
        "INSERT INTO follow_notice (kind, series_id, title, detail, at, read) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        [
            Value::Text(kind.to_string()),
            Value::Text(series_id.to_string()),
            Value::Text(title.to_string()),
            Value::Text(detail.to_string()),
            Value::Integer(at),
        ],
    )
    .await?;
    // last_insert_rowid 在同连接上读，串行 DB 线程内无竞争
    let mut rows = db
        .conn()
        .query("SELECT last_insert_rowid()", ())
        .await
        .map_err(map_db_err)?;
    let row = rows
        .next()
        .await
        .map_err(map_db_err)?
        .ok_or_else(|| AppError::StoreCorrupt("last_insert_rowid 无行".into()))?;
    col_i64(&row, 0)
}

/// 通知列表（新→旧；封顶 200 条——只进不出的表给个上界）。
pub async fn notices_list(db: &Db) -> AppResult<Vec<FollowNotice>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT id, kind, series_id, title, detail, at, read FROM follow_notice
             ORDER BY at DESC, id DESC LIMIT 200",
            (),
        )
        .await
        .map_err(map_db_err)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_db_err)? {
        out.push(FollowNotice {
            id: col_i64(&row, 0)?,
            kind: col_text(&row, 1)?,
            series_id: col_text(&row, 2)?,
            title: col_text(&row, 3)?,
            detail: col_text(&row, 4)?,
            at: col_i64(&row, 5)?,
            read: col_i64(&row, 6)? != 0,
        });
    }
    Ok(out)
}

/// 标记已读，返回改动的行数。
pub async fn notices_mark_read(db: &Db, ids: &[i64]) -> AppResult<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    // 参数个数固定的批量 UPDATE 拆成逐条——表有 200 条上界，循环开销可忽略
    let mut n = 0u64;
    for id in ids {
        n += db
            .execute(
                "UPDATE follow_notice SET read = 1 WHERE id = ?1 AND read = 0",
                [Value::Integer(*id)],
            )
            .await?;
    }
    Ok(n)
}

/// 清空通知。
pub async fn notices_clear(db: &Db) -> AppResult<()> {
    db.execute("DELETE FROM follow_notice", ()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{mem_db, rt};

    #[test]
    fn baseline_roundtrip_and_delete() {
        let db = mem_db();
        rt().block_on(async {
            assert_eq!(baseline_get(&db, "s1").await.unwrap(), None);
            baseline_set(&db, "s1", 214).await.unwrap();
            baseline_set(&db, "s1", 216).await.unwrap();
            assert_eq!(baseline_get(&db, "s1").await.unwrap(), Some(216));
            baseline_delete(&db, "s1").await.unwrap();
            assert_eq!(baseline_get(&db, "s1").await.unwrap(), None);
        });
    }

    #[test]
    fn notice_insert_list_read_clear() {
        let db = mem_db();
        rt().block_on(async {
            let a = notice_insert(&db, "follow", "s1", "剧A", "更新了 2 集", 100)
                .await
                .unwrap();
            let b = notice_insert(&db, "follow", "s2", "剧B", "更新了 1 集", 200)
                .await
                .unwrap();
            assert_ne!(a, b);

            let list = notices_list(&db).await.unwrap();
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].id, b, "新的在前");
            assert!(!list[0].read);

            assert_eq!(notices_mark_read(&db, &[a]).await.unwrap(), 1);
            assert_eq!(
                notices_mark_read(&db, &[a]).await.unwrap(),
                0,
                "重复标记计 0"
            );
            let list = notices_list(&db).await.unwrap();
            assert!(list.iter().find(|n| n.id == a).unwrap().read);

            notices_clear(&db).await.unwrap();
            assert!(notices_list(&db).await.unwrap().is_empty());
        });
    }
}
