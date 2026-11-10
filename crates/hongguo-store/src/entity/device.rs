//! device_profile 实体的 SQL 读写：现役/备份/bootstrap 元数据三行约定。

use turso::Value;

use crate::db::Db;
use hongguo_common::{AppError, AppResult};

use super::{col_text, map_db_err};

/// 表内行号约定：1=现役档案，2=上一代备份（换档前自动留存，供回查/
/// 回滚），10=bootstrap 元数据（轮换来源、上次注册尝试等，JSON 文档）。
const DEVICE_ROW_ACTIVE: i64 = 1;
const DEVICE_ROW_BACKUP: i64 = 2;
const DEVICE_ROW_META: i64 = 10;
const DEVICE_ROW_SDK_SESSION: i64 = 11;

pub async fn sdk_session(db: &Db) -> AppResult<Option<serde_json::Value>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT json FROM device_profile WHERE id = ?1",
            [Value::Integer(DEVICE_ROW_SDK_SESSION)],
        )
        .await
        .map_err(map_db_err)?;
    let Some(row) = rows.next().await.map_err(map_db_err)? else {
        return Ok(None);
    };
    serde_json::from_str(&col_text(&row, 0)?)
        .map(Some)
        .map_err(|_| AppError::StoreCorrupt("MetaSec session storage is invalid".into()))
}

pub async fn save_sdk_session(db: &Db, session: &serde_json::Value) -> AppResult<()> {
    let json = serde_json::to_string(session)
        .map_err(|_| AppError::StoreCorrupt("cannot encode MetaSec session storage".into()))?;
    db.execute(
        "INSERT INTO device_profile (id, json) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET json = excluded.json",
        [Value::Integer(DEVICE_ROW_SDK_SESSION), Value::Text(json)],
    )
    .await?;
    Ok(())
}

/// 读设备档案。从未注册过（表空）返回 None，调用方用静态兜底档案。
pub async fn device_profile(db: &Db) -> AppResult<Option<hongguo_signer::device::DeviceProfile>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT json FROM device_profile WHERE id = ?1",
            [Value::Integer(DEVICE_ROW_ACTIVE)],
        )
        .await
        .map_err(map_db_err)?;
    let Some(row) = rows.next().await.map_err(map_db_err)? else {
        return Ok(None);
    };
    let json = col_text(&row, 0)?;
    serde_json::from_str(&json)
        .map(Some)
        .map_err(|e| AppError::StoreCorrupt(format!("设备档案反序列化失败: {e}")))
}

/// 写设备档案（单行表）。
pub async fn save_device_profile(
    db: &Db,
    profile: &hongguo_signer::device::DeviceProfile,
) -> AppResult<()> {
    let json = serde_json::to_string(profile)
        .map_err(|e| AppError::StoreCorrupt(format!("设备档案序列化失败: {e}")))?;
    db.execute(
        "INSERT INTO device_profile (id, json) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET json = excluded.json",
        [Value::Integer(DEVICE_ROW_ACTIVE), Value::Text(json)],
    )
    .await?;
    Ok(())
}

/// 把现役档案复制到备份行。现役不存在时是空操作（返回 false）。
/// 换档（注册成功 / 阵亡重铺）前调用，「现役 + 备份」双档案形态。
pub async fn backup_device_profile(db: &Db) -> AppResult<bool> {
    let touched = db
        .execute(
            "INSERT INTO device_profile (id, json)
             SELECT ?1, json FROM device_profile WHERE id = ?2
             ON CONFLICT(id) DO UPDATE SET json = excluded.json",
            [
                Value::Integer(DEVICE_ROW_BACKUP),
                Value::Integer(DEVICE_ROW_ACTIVE),
            ],
        )
        .await?;
    Ok(touched > 0)
}

/// 读上一代备份档案（回查用；从没换过档返回 None）。
pub async fn device_profile_backup(
    db: &Db,
) -> AppResult<Option<hongguo_signer::device::DeviceProfile>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT json FROM device_profile WHERE id = ?1",
            [Value::Integer(DEVICE_ROW_BACKUP)],
        )
        .await
        .map_err(map_db_err)?;
    let Some(row) = rows.next().await.map_err(map_db_err)? else {
        return Ok(None);
    };
    let json = col_text(&row, 0)?;
    serde_json::from_str(&json)
        .map(Some)
        .map_err(|e| AppError::StoreCorrupt(format!("备份档案反序列化失败: {e}")))
}

/// 读 bootstrap 元数据文档（轮换来源、上次注册尝试；无则 None）。
pub async fn device_meta_get(db: &Db) -> AppResult<Option<serde_json::Value>> {
    let mut rows = db
        .conn()
        .query(
            "SELECT json FROM device_profile WHERE id = ?1",
            [Value::Integer(DEVICE_ROW_META)],
        )
        .await
        .map_err(map_db_err)?;
    let Some(row) = rows.next().await.map_err(map_db_err)? else {
        return Ok(None);
    };
    let json = col_text(&row, 0)?;
    serde_json::from_str(&json)
        .map(Some)
        .map_err(|e| AppError::StoreCorrupt(format!("设备元数据反序列化失败: {e}")))
}

/// 现役、备份、元数据一次提交。中途失败由调用方的事务回滚，不会留下半套档案。
pub async fn commit_device_swap(db: &Db, profile_json: &str, meta_json: &str) -> AppResult<()> {
    commit_device_swap_at(db, profile_json, meta_json, None).await
}

/// `fail_after` 为 1/2/3 时在对应写入后返回错误，供隔离库故障注入。生产路径传 `None`。
pub async fn commit_device_swap_at(
    db: &Db,
    profile_json: &str,
    meta_json: &str,
    fail_after: Option<u8>,
) -> AppResult<()> {
    let profile_json = profile_json.to_string();
    let meta_json = meta_json.to_string();
    db.with_tx(move |conn| {
        let profile_json = profile_json.clone();
        let meta_json = meta_json.clone();
        async move {
            conn.execute(
                "INSERT INTO device_profile (id, json)
                 SELECT ?1, json FROM device_profile WHERE id = ?2
                 ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                [
                    Value::Integer(DEVICE_ROW_BACKUP),
                    Value::Integer(DEVICE_ROW_ACTIVE),
                ],
            )
            .await
            .map_err(map_db_err)?;
            if fail_after == Some(1) {
                return Err(AppError::StoreCorrupt("故障注入：备份之后".into()));
            }
            conn.execute(
                "INSERT INTO device_profile (id, json) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                [Value::Integer(DEVICE_ROW_ACTIVE), Value::Text(profile_json)],
            )
            .await
            .map_err(map_db_err)?;
            if fail_after == Some(2) {
                return Err(AppError::StoreCorrupt("故障注入：现役之后".into()));
            }
            conn.execute(
                "INSERT INTO device_profile (id, json) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                [Value::Integer(DEVICE_ROW_META), Value::Text(meta_json)],
            )
            .await
            .map_err(map_db_err)?;
            if fail_after == Some(3) {
                return Err(AppError::StoreCorrupt("故障注入：元数据之后".into()));
            }
            Ok(())
        }
    })
    .await
}

/// 写 bootstrap 元数据文档（整文档覆盖）。
pub async fn device_meta_set(db: &Db, meta: &serde_json::Value) -> AppResult<()> {
    let json = serde_json::to_string(meta)
        .map_err(|e| AppError::StoreCorrupt(format!("设备元数据序列化失败: {e}")))?;
    db.execute(
        "INSERT INTO device_profile (id, json) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET json = excluded.json",
        [Value::Integer(DEVICE_ROW_META), Value::Text(json)],
    )
    .await?;
    Ok(())
}
