//! 起点决策：这一剧从第几集、什么位置开始播。
//!
//! 「看过 > [`RESUME_THRESHOLD_SECS`]」的单一实现，本地断点（playback
//! 表，播放期 5 秒一写）与云端观看历史（约 1 分钟一报）**谁更新用谁**：
//! 本地只覆盖本应用（跨端进度滞后甚至缺失），云端跨端但有延迟，比较
//! `updated_at` 取新者——追更页「识别到 80 集、播放回第 1 集」这类跨端
//! 进度问题在这一处根治。都没有回落第 1 集。此前沉浸流起播与下一剧预取
//! 各写一份、前端详情页/首页又各一份——阈值漂移和口径分叉就是这么来的。

use hongguo_api::history::WatchHistoryItem;
use hongguo_common::AppResult;
use hongguo_common::model::playback::RESUME_THRESHOLD_SECS;
use hongguo_store::Store;

/// 片尾保护：续播落点最多到 `duration - ε`，避免 AVPlayer 起播即尾、
/// 尾帧不再推进导致 Ended 永不发生。0.5s 足够让解码器吐出尾帧并走完
/// `position >= duration - 0.25` 的结束判定。
const RESUME_TAIL_EPSILON_SECS: f64 = 0.5;

/// 起点决策。返回 `(集号, 起播秒)`；无断点时 `(1, 0.0)`。
///
/// 云端行的 vid_index 0/1 基混杂（第三方上报 0 基），按既有口径对 0 钳 1。
pub fn resolve_entry(
    store: &Store,
    series_id: &str,
    history: &[WatchHistoryItem],
) -> AppResult<(u32, f64)> {
    let local = store
        .series_last_position(series_id)?
        .filter(|(_, p)| p.is_resumable())
        .map(|(idx, p)| (idx, p.current_time, p.duration, p.updated_at));
    let threshold_ms = (RESUME_THRESHOLD_SECS * 1000.0) as i64;
    let cloud = history
        .iter()
        .find(|h| h.series_id == series_id && h.position_ms > threshold_ms);
    let (vid_index, start_secs, duration) = decide_entry(local, cloud);
    // 落点 clamp 在**决策层**做一次：所有调用方（首页/历史/详情/预取）
    // 都拿到同一个不会贴在 duration 上的起点。
    Ok((vid_index, clamp_tail(start_secs, duration)))
}

/// 纯决策（`resolve_entry` 的可测内核，`local` = `(集号, 起播秒, 时长, 更新时刻)`）：
/// 本地与云端都过了「>3 秒才算看过」闸后，比较更新时间取新者；同刻
/// （理论罕见）本地的集内位置更新，归本地。
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

/// 片尾夹取：duration 未知（0）时原样；已知时最多到 `duration - ε`。
fn clamp_tail(start_secs: f64, duration: f64) -> f64 {
    if duration > RESUME_TAIL_EPSILON_SECS {
        start_secs.clamp(0.0, duration - RESUME_TAIL_EPSILON_SECS)
    } else {
        start_secs.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(idx: u32, secs: f64, duration: f64, updated_at: i64) -> Option<(u32, f64, f64, i64)> {
        Some((idx, secs, duration, updated_at))
    }

    fn cloud(idx: i64, pos_ms: i64, duration_ms: i64, updated_at_ms: i64) -> WatchHistoryItem {
        WatchHistoryItem {
            series_id: "s".into(),
            title: String::new(),
            cover: String::new(),
            vid_index: idx,
            vid: String::new(),
            position_ms: pos_ms,
            duration_ms,
            episode_cnt: 0,
            updated_at_ms,
        }
    }

    #[test]
    fn cloud_newer_wins_cross_device_progress() {
        // issue #11 场景：本应用档案停在 7 集（上周），官方 App 看到 80 集
        // （今天）——取云端
        let got = decide_entry(
            local(7, 120.0, 180.0, 1_000),
            Some(&cloud(80, 30_000, 180_000, 2_000)),
        );
        assert_eq!(got, (80, 30.0, 180.0));
    }

    #[test]
    fn local_newer_wins_same_app_freshness() {
        // 只在本应用看：本地 5 秒一写永远比云端（约 1 分钟一报）新，
        // 不被滞后的云端把集号拽回去
        let got = decide_entry(
            local(51, 10.0, 180.0, 2_000),
            Some(&cloud(50, 120_000, 180_000, 1_000)),
        );
        assert_eq!(got, (51, 10.0, 180.0));
    }

    #[test]
    fn cloud_only_entry_clamps_zero_based_to_ep1() {
        let got = decide_entry(None, Some(&cloud(0, 30_000, 180_000, 1_000)));
        assert_eq!(got, (1, 30.0, 180.0), "0 基记录钳第 1 集，位置保留");
    }

    #[test]
    fn local_only_entry_unchanged() {
        let got = decide_entry(local(12, 45.0, 180.0, 1_000), None);
        assert_eq!(got, (12, 45.0, 180.0));
    }

    #[test]
    fn no_entries_falls_back_to_ep1() {
        assert_eq!(decide_entry(None, None), (1, 0.0, 0.0));
    }

    #[test]
    fn tail_start_is_clamped_before_player_open() {
        assert_eq!(clamp_tail(172.6, 172.6), 172.1);
        assert_eq!(clamp_tail(200.0, 172.6), 172.1);
        assert_eq!(clamp_tail(172.4, 172.6), 172.1);
        assert_eq!(clamp_tail(30.0, 172.6), 30.0);
        assert_eq!(clamp_tail(30.0, 0.0), 30.0, "时长未知不猜");
    }
}
