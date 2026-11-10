//! 实体读写：device / follow / local_history / merge / playback / series / settings / task 八个模块的 SQL 实现。设计约定：复杂实体（任务/剧集/合并）整行存 JSON 列，只把需要 WHERE / ORDER / JOIN 的字段提升为列；playback 是最热写入点（前端 5 秒节流上报），全真实列 + UPSERT；所有多行写操作包在单个事务里。
//!
//! 本模块全部是 `async fn`，跑在 [`crate::bridge`] 的专用 DB 线程上、由同步门面 [`crate::Store`] 逐方法转调；不要在业务代码里直接调用。

use hongguo_common::{AppError, AppResult};

#[cfg(test)]
use crate::db::Db;

fn col_text(row: &turso::Row, i: usize) -> AppResult<String> {
    row.get_value(i)
        .ok()
        .and_then(|v| v.as_text().cloned())
        .ok_or_else(|| AppError::StoreCorrupt(format!("第 {i} 列不是文本")))
}

fn col_i64(row: &turso::Row, i: usize) -> AppResult<i64> {
    row.get_value(i)
        .ok()
        .and_then(|v| v.as_integer().copied())
        .ok_or_else(|| AppError::StoreCorrupt(format!("第 {i} 列不是整型")))
}

fn col_f64(row: &turso::Row, i: usize) -> AppResult<f64> {
    row.get_value(i)
        .ok()
        .and_then(|v| v.as_real().copied())
        .ok_or_else(|| AppError::StoreCorrupt(format!("第 {i} 列不是浮点")))
}

/// 枚举状态序列化成落库字符串（"pending" / "running" / …）。
fn status_text<T: serde::Serialize>(status: &T) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn map_db_err(e: turso::Error) -> AppError {
    AppError::StoreCorrupt(format!("SQL 失败: {e}"))
}

mod device;
mod follow;
mod local_history;
mod merge;
mod playback;
mod series;
mod settings;
mod task;

pub use device::{
    backup_device_profile, commit_device_swap, commit_device_swap_at, device_meta_get,
    device_meta_set, device_profile, device_profile_backup, save_device_profile, save_sdk_session,
    sdk_session,
};
pub use follow::{
    baseline_delete, baseline_get, baseline_set, notice_insert, notices_clear, notices_list,
    notices_mark_read,
};
pub use local_history::{
    delete_local_history, local_history_list, mark_local_history_reported, pending_local_history,
    upsert_local_history,
};
pub use merge::{delete_merge_task, merge_tasks, upsert_merge_task};
pub use playback::{playback_position, save_playback_position, series_last_position};
pub use series::{series_all, series_by_id, upsert_series};
pub use settings::{save_settings, settings};
pub use task::{replace_tasks, tasks};

#[cfg(test)]
fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("测试 runtime")
}

#[cfg(test)]
fn mem_db() -> Db {
    rt().block_on(Db::open(":memory:")).expect("内存库")
}

#[cfg(test)]
mod tests {
    use super::*;

    use hongguo_common::model::TaskStatus;

    #[test]
    fn task_status_column_matches_model() {
        assert_eq!(status_text(&TaskStatus::Running), "running");
    }
}
