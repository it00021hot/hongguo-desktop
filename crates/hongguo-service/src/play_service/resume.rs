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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_entry_compares_real_stored_progress_without_the_native_feed_tail_rule() {
        let store = Store::open_memory().unwrap();
        let local = hongguo_common::model::PlaybackPosition {
            current_time: 12.0,
            duration: 100.0,
            updated_at: 1_000,
        };
        store.save_playback_position("s", 7, &local).unwrap();
        let newer = cloud(9, 20_000, 20_000, 2_000);
        assert_eq!(
            resolve_recent_entry(&store, "s", std::slice::from_ref(&newer)).unwrap(),
            (9, 20.0)
        );
        let latest = hongguo_common::model::PlaybackPosition {
            current_time: 15.0,
            updated_at: 3_000,
            ..local
        };
        store.save_playback_position("s", 12, &latest).unwrap();
        assert_eq!(
            resolve_recent_entry(&store, "s", &[cloud(9, 20_000, 100_000, 2_000)]).unwrap(),
            (12, 15.0)
        );
        store
            .save_playback_position(
                "s",
                14,
                &hongguo_common::model::PlaybackPosition {
                    current_time: 3.0,
                    updated_at: 4_000,
                    ..local
                },
            )
            .unwrap();
        assert_eq!(resolve_recent_entry(&store, "s", &[]).unwrap(), (1, 0.0));
    }

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
        let got = decide_entry(
            local(7, 120.0, 180.0, 1_000),
            Some(&cloud(80, 30_000, 180_000, 2_000)),
        );
        assert_eq!(got, (80, 30.0, 180.0));
    }

    #[test]
    fn local_newer_wins_same_app_freshness() {
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
}
