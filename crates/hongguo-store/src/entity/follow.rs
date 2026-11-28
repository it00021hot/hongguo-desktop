//! follow_baseline / follow_notice 的 SQL 读写（追更检查的本地两件套）。

use turso::Value;

use crate::db::Db;
use hongguo_common::model::{FollowNotice, NOTICE_KIND_FOLLOW};
use hongguo_common::{AppError, AppResult};

use super::{col_i64, col_text, map_db_err};

/// 某剧的基线集数；无基线（首次见到）返回 `None`。
async fn baseline_get(db: &Db, series_id: &str) -> AppResult<Option<i64>> {
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
         ON CONFLICT(series_id) DO UPDATE SET episode_cnt = excluded.episode_cnt, updated_at = excluded.updated_at
         WHERE excluded.episode_cnt > follow_baseline.episode_cnt",
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
async fn notice_insert(
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowObservation {
    pub series_id: String,
    pub title: String,
    pub episode_count: i64,
}

/// Compare and advance a page atomically. Overlapping manual/automatic scans
/// cannot observe the same old baseline between a notice and its update.
pub async fn observe_page(db: &Db, items: Vec<FollowObservation>) -> AppResult<Vec<FollowNotice>> {
    let at = chrono::Utc::now().timestamp();
    db.with_tx(move |_| async move {
        let mut fresh = Vec::new();
        for item in items {
            if item.series_id.is_empty() || item.episode_count <= 0 {
                continue;
            }
            let Some(previous) = baseline_get(db, &item.series_id)
                .await?
                .filter(|count| *count > 0)
            else {
                baseline_set(db, &item.series_id, item.episode_count).await?;
                continue;
            };
            if item.episode_count <= previous {
                continue;
            }
            let added = item.episode_count - previous;
            let title = if item.title.is_empty() {
                "追更更新".to_owned()
            } else {
                item.title
            };
            let detail = format!(
                "追的剧更新了，新增 {added} 集（共 {} 集）",
                item.episode_count
            );
            let id =
                notice_insert(db, NOTICE_KIND_FOLLOW, &item.series_id, &title, &detail, at).await?;
            baseline_set(db, &item.series_id, item.episode_count).await?;
            fresh.push(FollowNotice {
                id,
                kind: NOTICE_KIND_FOLLOW.into(),
                series_id: item.series_id,
                title,
                detail,
                at,
                read: false,
            });
        }
        Ok(fresh)
    })
    .await
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
