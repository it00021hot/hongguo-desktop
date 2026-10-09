//! playback 实体的 SQL 读写：最热写路径，全真实列 + 单行 UPSERT。

use turso::Value;

use crate::domain::model::PlaybackPosition;
use crate::error::AppResult;
use crate::store::db::Db;

use super::{col_f64, col_i64, map_db_err};

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
            [
                Value::Text(series_id.to_string()),
                Value::Integer(vid_index as i64),
            ],
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

/// 读一部剧「最近看到的那一集」及其位置（按 updated_at 取最新一行）。
///
/// 详情页「继续看第 N 集」的真值来源：本地 playback 表 5 秒一写，
/// 永远比云端观看历史（约 1 分钟一报 + 接口缓存）新鲜。
pub async fn series_last_position(
    db: &Db,
    series_id: &str,
) -> AppResult<Option<(u32, PlaybackPosition)>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT vid_index, \"current_time\", duration, updated_at FROM playback
             WHERE series_id = ?1 ORDER BY updated_at DESC LIMIT 1",
            [Value::Text(series_id.to_string())],
        )
        .await
        .map_err(map_db_err)?;
    let Some(row) = rows.next().await.map_err(map_db_err)? else {
        return Ok(None);
    };
    Ok(Some((
        col_i64(&row, 0)?.max(0) as u32,
        PlaybackPosition {
            current_time: col_f64(&row, 1)?,
            duration: col_f64(&row, 2)?,
            updated_at: col_i64(&row, 3)?,
        },
    )))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::entity::{mem_db, rt};

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
                playback_position(&db, "A", 1)
                    .await
                    .unwrap()
                    .map(|p| p.current_time),
                Some(20.0)
            );
            assert_eq!(
                playback_position(&db, "A", 2)
                    .await
                    .unwrap()
                    .map(|p| p.current_time),
                Some(50.0),
                "另一集的进度互不干扰"
            );
        });
    }
}
