//! 起点决策：这一剧从第几集、什么位置开始播。「看过 > [`RESUME_THRESHOLD_SECS`]」的单一实现。
//! 本地断点（播放期 5 秒一写）与云端观看历史（约 1 分钟一报）**谁更新用谁**——本地只覆盖
//! 本应用，云端跨端但有延迟，比较 `updated_at` 取新者；都没有回落第 1 集。

use hongguo_api::history::WatchHistoryItem;
use hongguo_common::AppResult;
use hongguo_common::model::playback::RESUME_THRESHOLD_SECS;
use hongguo_store::Store;

/// 起点决策，返回 `(集号, 起播秒)`；无断点时 `(1, 0.0)`。云端行的 vid_index 0/1 基混杂
///（早期客户端上报 0 基），按既有口径对 0 钳 1。
pub fn resolve_recent_entry(
    store: &Store,
    series_id: &str,
    history: &[WatchHistoryItem],
) -> AppResult<(u32, f64)> {
    recent_entry(store, series_id, history).map(|(index, at, _)| (index, at))
}

fn recent_entry(
    store: &Store,
    series_id: &str,
    history: &[WatchHistoryItem],
) -> AppResult<(u32, f64, f64)> {
    let local = store
        .series_last_position(series_id)?
        .filter(|(_, p)| p.is_resumable())
        .map(|(idx, p)| (idx, p.current_time, p.duration, p.updated_at));
    let threshold_ms = (RESUME_THRESHOLD_SECS * 1000.0) as i64;
    let cloud = history
        .iter()
        .find(|h| h.series_id == series_id && h.position_ms > threshold_ms);
    Ok(decide_entry(local, cloud))
}

/// 纯决策（`resolve_recent_entry` 的可测内核，`local` = `(集号, 起播秒, 时长, 更新时刻)`）：
/// 本地与云端都过「看过」闸后比较更新时间取新者；同刻（理论罕见）本地的集内位置更新，归本地。
fn decide_entry(
    local: Option<(u32, f64, f64, i64)>,
    cloud: Option<&WatchHistoryItem>,
) -> (u32, f64, f64) {
    match (local, cloud) {
        (Some((_, _, _, updated_at)), Some(h)) if h.updated_at_ms > updated_at => (
            h.vid_index.max(1) as u32,
            h.position_ms as f64 / 1000.0,
            h.duration_ms as f64 / 1000.0,
        ),
        (Some(entry), _) => (entry.0, entry.1, entry.2),
        (None, Some(h)) => (
            h.vid_index.max(1) as u32,
            h.position_ms as f64 / 1000.0,
            h.duration_ms as f64 / 1000.0,
        ),
        (None, None) => (1, 0.0, 0.0),
    }
}
