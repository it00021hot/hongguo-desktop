//! 断点续播读写。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::PlaybackPosition;

/// 保存播放位置。
///
/// `duration` 由前端回传——后端拿到流的时候还不知道总时长，
/// 而「接近片尾就不续播」这条判断依赖它，不记就等于这道防线一直是空转的。
pub fn save(
    state: &State<'_, AppState>,
    series_id: &str,
    vid_index: u32,
    current_time: f64,
    duration: f64,
) -> crate::error::AppResult<()> {
    let mut data = state.store.write();
    let entry = data.playback.entry(series_id.to_string()).or_default();
    entry.insert(vid_index, PlaybackPosition::new(current_time, duration));
    data.save(&crate::store::paths::data_file())
        .map_err(|e| crate::error::AppError::StoreCorrupt(e.to_string()))
}

/// 读播放位置。接近片尾时返回 0（从头看）。
pub fn load(state: &State<'_, AppState>, series_id: &str, vid_index: u32) -> f64 {
    let data = state.store.read();
    data.playback
        .get(series_id)
        .and_then(|m| m.get(&vid_index))
        .filter(|p| !p.is_near_end())
        .map(|p| p.current_time)
        .unwrap_or(0.0)
}

/// 播放历史：每部剧最近一次看到的位置，按时间倒序。
///
/// 只给「最近一集」而不是全部集次：列表要回答的是「我播过哪些剧、看到哪」，
/// 逐集罗列反而看不出重点。
pub fn history(state: &State<'_, AppState>) -> Vec<crate::domain::model::PlaybackHistoryItem> {
    history_of(&state.store.read().playback)
}

/// 清除某部剧的观看记录（整部剧的进度表都删掉，不只是最近那一集）。
///
/// 历史列表每部剧只显示最近一集，但进度表里存着所有看过的集次。
/// 只删最近一集的话，下一次打开又会把更早的那一集顶上来，用户会以为没删掉。
pub fn remove(state: &AppState, series_id: &str) -> crate::error::AppResult<()> {
    let mut data = state.store.write();
    // 走历史列表的剧可能还没被登记成档案，但进度表里一定有；找不到就说明本来就没有
    if data.playback.remove(series_id).is_none() {
        return Err(crate::error::AppError::NotFound(format!(
            "观看记录 {series_id}"
        )));
    }
    data.save(&crate::store::paths::data_file())
        .map_err(|e| crate::error::AppError::StoreCorrupt(e.to_string()))
}

/// 清空全部播放历史。
///
/// 空历史重复清空不算错误：这是一个「清掉」按钮，前端可能连点两次。
pub fn clear(state: &AppState) -> crate::error::AppResult<()> {
    let mut data = state.store.write();
    data.playback.clear();
    data.save(&crate::store::paths::data_file())
        .map_err(|e| crate::error::AppError::StoreCorrupt(e.to_string()))
}

/// 从播放进度表里取每部剧最近一集，按时间倒序。
fn history_of(
    map: &crate::domain::model::PlaybackMap,
) -> Vec<crate::domain::model::PlaybackHistoryItem> {
    use crate::domain::model::PlaybackHistoryItem;

    let mut items: Vec<PlaybackHistoryItem> = map
        .iter()
        .filter_map(|(series_id, episodes)| {
            let (vid_index, pos) = episodes
                .iter()
                .max_by_key(|(_, p)| p.updated_at)
                .map(|(idx, p)| (*idx, p))?;
            Some(PlaybackHistoryItem {
                series_id: series_id.clone(),
                vid_index,
                current_time: pos.current_time,
                updated_at: pos.updated_at,
            })
        })
        .collect();
    items.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{PlaybackMap, PlaybackPosition};
    use std::path::PathBuf;

    fn at(current_time: f64, updated_at: i64) -> PlaybackPosition {
        PlaybackPosition {
            updated_at,
            ..PlaybackPosition::new(current_time, 300.0)
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-play-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn picks_the_most_recent_episode_per_series() {
        let mut map = PlaybackMap::new();
        map.entry("A".into()).or_default().insert(1, at(10.0, 100));
        map.entry("A".into()).or_default().insert(7, at(70.0, 900));
        map.get_mut("A").unwrap().insert(3, at(30.0, 500));

        let items = history_of(&map);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].series_id, "A");
        assert_eq!(items[0].vid_index, 7, "应取更新时间最晚的那一集");
        assert_eq!(items[0].current_time, 70.0);
    }

    #[test]
    fn sorts_series_by_last_watched_desc() {
        let mut map = PlaybackMap::new();
        map.entry("A".into()).or_default().insert(1, at(10.0, 100));
        map.entry("B".into()).or_default().insert(2, at(20.0, 900));
        map.entry("C".into()).or_default().insert(3, at(30.0, 500));

        let items = history_of(&map);
        assert_eq!(
            items
                .iter()
                .map(|i| i.series_id.as_str())
                .collect::<Vec<_>>(),
            ["B", "C", "A"]
        );
    }

    #[test]
    fn empty_map_gives_empty_history() {
        assert!(history_of(&PlaybackMap::new()).is_empty());
    }

    #[test]
    fn clear_empties_history_on_disk_too() {
        let dir = temp_dir("clear");
        let _scoped = crate::store::paths::ScopedDataDir::new(&dir);

        let state = AppState::default();
        state
            .store
            .write()
            .playback
            .entry("A".into())
            .or_default()
            .insert(1, at(10.0, 100));
        // 先把「有记录」的状态写进文件：否则「文件里是空的」这句断言没有对照，
        // 清空没落盘它也一样成立
        state
            .store
            .read()
            .save(&crate::store::paths::data_file())
            .unwrap();
        assert!(
            !crate::store::DataStore::load(&crate::store::paths::data_file())
                .playback
                .is_empty()
        );

        clear(&state).expect("清空历史不该失败");

        assert!(
            history_of(&state.store.read().playback).is_empty(),
            "清空后不该再有历史（history() 读的就是这份表）"
        );
        let on_disk = crate::store::DataStore::load(&crate::store::paths::data_file());
        assert!(
            on_disk.playback.is_empty(),
            "只清内存的话，重启后历史全回来了"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clearing_empty_history_is_a_no_op() {
        let dir = temp_dir("clear-empty");
        let _scoped = crate::store::paths::ScopedDataDir::new(&dir);

        let state = AppState::default();
        clear(&state).expect("空历史重复清空不该报错");
        assert!(history_of(&state.store.read().playback).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_drops_the_whole_series_not_just_the_latest_episode() {
        let dir = temp_dir("remove-one");
        let _scoped = crate::store::paths::ScopedDataDir::new(&dir);

        let state = AppState::default();
        {
            let mut map = state.store.write();
            for idx in [1u32, 5, 9] {
                map.playback
                    .entry("A".into())
                    .or_default()
                    .insert(idx, at(1.0, idx as i64));
            }
            map.playback
                .entry("B".into())
                .or_default()
                .insert(1, at(1.0, 1));
        }

        remove(&state, "A").unwrap();

        let store = state.store.read();
        assert!(
            !store.playback.contains_key("A"),
            "整部剧都要清掉，否则下一集会顶上来，用户以为没删"
        );
        assert!(store.playback.contains_key("B"), "别的剧不受影响");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_reports_not_found_for_unknown_series() {
        let dir = temp_dir("remove-missing");
        let _scoped = crate::store::paths::ScopedDataDir::new(&dir);

        let state = AppState::default();
        assert!(remove(&state, "ZZZ").is_err(), "没有记录就不能假装删成功");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
