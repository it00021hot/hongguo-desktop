//! tasks 实体的 SQL 读写：整行 JSON + 状态列，快照整体替换走单事务。

use turso::{Connection, Value};

use crate::domain::model::DownloadTask;
use crate::error::{AppError, AppResult};
use crate::store::db::Db;

use super::{col_text, map_db_err, status_text};

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
        conn.execute("DELETE FROM tasks", ())
            .await
            .map_err(map_db_err)?;
        insert_tasks(conn, snapshot).await?;
        Ok(())
    })
    .await
}

pub(super) async fn insert_tasks(conn: &Connection, tasks: &[DownloadTask]) -> AppResult<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::entity::{mem_db, rt};

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
}
