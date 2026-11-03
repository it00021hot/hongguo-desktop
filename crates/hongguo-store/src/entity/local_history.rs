//! 本地观看历史的 SQL 读写：每剧一行（PRIMARY KEY series_id，行 = 最近看的那一集），与云端 read_history 的按剧聚合口径一致。
//!
//! `reported` 标记该行是否已上报云端：UPSERT 一律清 0（写入即视为待同步），登录态上报成功后由 service 回置 1。

use turso::Value;

use crate::db::Db;
use hongguo_common::AppResult;
use hongguo_common::model::LocalHistoryItem;

use super::{col_i64, col_text, map_db_err};

/// UPSERT 一行（每剧一行）。任何写入都把 reported 清 0，登录态的云端
/// 上报成功后由 [`mark_local_history_reported`] 回置。
pub async fn upsert_local_history(db: &Db, item: &LocalHistoryItem) -> AppResult<()> {
    db.conn()
        .execute(
            "INSERT INTO local_history
                 (series_id, vid_index, vid, title, cover, episode_count,
                  position_ms, duration_ms, updated_at, reported)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
             ON CONFLICT(series_id) DO UPDATE SET
                 vid_index = excluded.vid_index,
                 vid = excluded.vid,
                 title = excluded.title,
                 cover = excluded.cover,
                 episode_count = excluded.episode_count,
                 position_ms = excluded.position_ms,
                 duration_ms = excluded.duration_ms,
                 updated_at = excluded.updated_at,
                 reported = 0",
            [
                Value::Text(item.series_id.clone()),
                Value::Integer(item.vid_index as i64),
                Value::Text(item.vid.clone()),
                Value::Text(item.title.clone()),
                Value::Text(item.cover.clone()),
                Value::Integer(item.episode_cnt as i64),
                Value::Integer(item.position_ms),
                Value::Integer(item.duration_ms),
                Value::Integer(item.updated_at_ms),
            ],
        )
        .await
        .map_err(map_db_err)?;
    Ok(())
}

/// 全量本地历史，最近观看在前（历史页两种模式共用卡片渲染）。
pub async fn local_history_list(db: &Db) -> AppResult<Vec<LocalHistoryItem>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT series_id, title, cover, vid_index, vid, position_ms,
                    duration_ms, episode_count, updated_at
             FROM local_history ORDER BY updated_at DESC",
            (),
        )
        .await
        .map_err(map_db_err)?;
    let mut items = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_db_err)? {
        items.push(LocalHistoryItem {
            series_id: col_text(&row, 0)?,
            title: col_text(&row, 1)?,
            cover: col_text(&row, 2)?,
            vid_index: col_i64(&row, 3)?.max(0) as u32,
            vid: col_text(&row, 4)?,
            position_ms: col_i64(&row, 5)?,
            duration_ms: col_i64(&row, 6)?,
            episode_cnt: col_i64(&row, 7)?.max(0) as u32,
            updated_at_ms: col_i64(&row, 8)?,
        });
    }
    Ok(items)
}

/// 批量删除（按剧；历史页的卡片/批量管理以剧为单位）。
pub async fn delete_local_history(db: &Db, series_ids: &[String]) -> AppResult<()> {
    if series_ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; series_ids.len()].join(", ");
    let sql = format!("DELETE FROM local_history WHERE series_id IN ({placeholders})");
    let params: Vec<Value> = series_ids
        .iter()
        .map(|id| Value::Text(id.clone()))
        .collect();
    db.conn().execute(&sql, params).await.map_err(map_db_err)?;
    Ok(())
}

/// 待补传的行（reported = 0），最近优先；`limit` 防单轮刷爆。
pub async fn pending_local_history(db: &Db, limit: usize) -> AppResult<Vec<LocalHistoryItem>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT series_id, title, cover, vid_index, vid, position_ms,
                    duration_ms, episode_count, updated_at
             FROM local_history WHERE reported = 0
             ORDER BY updated_at DESC LIMIT ?1",
            [Value::Integer(limit as i64)],
        )
        .await
        .map_err(map_db_err)?;
    let mut items = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_db_err)? {
        items.push(LocalHistoryItem {
            series_id: col_text(&row, 0)?,
            title: col_text(&row, 1)?,
            cover: col_text(&row, 2)?,
            vid_index: col_i64(&row, 3)?.max(0) as u32,
            vid: col_text(&row, 4)?,
            position_ms: col_i64(&row, 5)?,
            duration_ms: col_i64(&row, 6)?,
            episode_cnt: col_i64(&row, 7)?.max(0) as u32,
            updated_at_ms: col_i64(&row, 8)?,
        });
    }
    Ok(items)
}

/// 上报成功后回置标记。
pub async fn mark_local_history_reported(db: &Db, series_id: &str) -> AppResult<()> {
    db.conn()
        .execute(
            "UPDATE local_history SET reported = 1 WHERE series_id = ?1",
            [Value::Text(series_id.to_string())],
        )
        .await
        .map_err(map_db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{mem_db, rt};
    use super::*;

    fn item(series_id: &str, vid_index: u32) -> LocalHistoryItem {
        LocalHistoryItem {
            series_id: series_id.into(),
            title: "剧".into(),
            cover: "cover".into(),
            vid_index,
            vid: "vid".into(),
            position_ms: 1000,
            duration_ms: 60_000,
            episode_cnt: 18,
            updated_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn upsert_keeps_one_row_per_series_latest_episode() {
        let db = mem_db();
        rt().block_on(async {
            upsert_local_history(&db, &item("s1", 3)).await.unwrap();
            let mut second = item("s1", 5);
            second.updated_at_ms += 1;
            upsert_local_history(&db, &second).await.unwrap();
            let list = local_history_list(&db).await.unwrap();
            assert_eq!(list.len(), 1, "每剧一行");
            assert_eq!(list[0].vid_index, 5, "行 = 最近看的那集");
            assert_eq!(list[0].updated_at_ms, second.updated_at_ms);
        });
    }

    #[test]
    fn upsert_resets_reported_and_mark_flips_it() {
        let db = mem_db();
        rt().block_on(async {
            upsert_local_history(&db, &item("s1", 1)).await.unwrap();
            assert!(pending_local_history(&db, 10).await.unwrap().len() == 1);
            mark_local_history_reported(&db, "s1").await.unwrap();
            assert!(pending_local_history(&db, 10).await.unwrap().is_empty());
            upsert_local_history(&db, &item("s1", 2)).await.unwrap();
            assert_eq!(pending_local_history(&db, 10).await.unwrap().len(), 1);
        });
    }

    #[test]
    fn delete_removes_only_named_series() {
        let db = mem_db();
        rt().block_on(async {
            upsert_local_history(&db, &item("s1", 1)).await.unwrap();
            upsert_local_history(&db, &item("s2", 1)).await.unwrap();
            delete_local_history(&db, &["s1".into()]).await.unwrap();
            let list = local_history_list(&db).await.unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].series_id, "s2");
        });
    }

    #[test]
    fn pending_respects_limit_and_order() {
        let db = mem_db();
        rt().block_on(async {
            for i in 0..5u32 {
                let mut it = item(&format!("s{i}"), 1);
                it.updated_at_ms += i as i64;
                upsert_local_history(&db, &it).await.unwrap();
            }
            let pending = pending_local_history(&db, 3).await.unwrap();
            assert_eq!(pending.len(), 3);
            assert_eq!(pending[0].series_id, "s4", "最近优先");
        });
    }
}
