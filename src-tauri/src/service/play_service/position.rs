//! 断点续播读写。
//!
//! 存储切到数据库后这里全是对 [`Store`] 的单行/单表操作：
//! 保存是单行 UPSERT（播放期间每 5 秒一次也不再有整文件重写），
//! 历史是「每部剧最新一集」的聚合查询 + 档案表取剧名封面。

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
    state.store.save_playback_position(
        series_id,
        vid_index,
        &PlaybackPosition::new(current_time, duration),
    )
}

/// 读播放位置。接近片尾时返回 0（从头看）。
pub fn load(state: &State<'_, AppState>, series_id: &str, vid_index: u32) -> f64 {
    state
        .store
        .playback_position(series_id, vid_index)
        .ok()
        .flatten()
        .filter(|p| !p.is_near_end())
        .map(|p| p.current_time)
        .unwrap_or(0.0)
}

/// 播放历史：每部剧最近一次看到的位置，按时间倒序。
///
/// 只给「最近一集」而不是全部集次：列表要回答的是「我播过哪些剧、看到哪」，
/// 逐集罗列反而看不出重点。
///
/// 剧名与封面在这里一并带出，**不过滤 dismissed**：观看记录回答的是「我看过
/// 什么」，和「剧集列表里还留着这部剧」是两件事。让前端拿历史去关联剧集列表，
/// 会导致用户从列表里移除一部剧就把它的观看记录一起抹掉。
pub fn history(state: &State<'_, AppState>) -> Vec<crate::domain::model::PlaybackHistoryItem> {
    let Ok(latest) = state.store.playback_latest_per_series() else {
        return Vec::new();
    };
    // 档案缺失就留空串：进度还在，条目照样要显示，只是没封面没剧名。
    let titles: std::collections::HashMap<String, (String, String)> = state
        .store
        .series_all()
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.series_id, (s.title, s.cover)))
        .collect();
    latest
        .into_iter()
        .map(|row| {
            let (title, cover) = titles
                .get(&row.series_id)
                .cloned()
                .unwrap_or((String::new(), String::new()));
            crate::domain::model::PlaybackHistoryItem {
                series_id: row.series_id,
                vid_index: row.vid_index,
                current_time: row.position.current_time,
                updated_at: row.position.updated_at,
                title,
                cover,
            }
        })
        .collect()
}

/// 清除某部剧的观看记录（整部剧的进度表都删掉，不只是最近那一集）。
///
/// 历史列表每部剧只显示最近一集，但进度表里存着所有看过的集次。
/// 只删最近一集的话，下一次打开又会把更早的那一集顶上来，用户会以为没删掉。
pub fn remove(state: &AppState, series_id: &str) -> crate::error::AppResult<()> {
    if state.store.remove_playback(series_id)? == 0 {
        return Err(crate::error::AppError::NotFound(format!(
            "观看记录 {series_id}"
        )));
    }
    Ok(())
}

/// 清空全部播放历史。
///
/// 空历史重复清空不算错误：这是一个「清掉」按钮，前端可能连点两次。
pub fn clear(state: &AppState) -> crate::error::AppResult<()> {
    state.store.clear_playback()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{PlaybackPosition, Series};
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

    /// 用磁盘文件库构造状态：历史/清空这类断言要验证「真的写进了库」，
    /// 内存库与 AppState::default 够不到「重启还在」这条语义。
    fn file_state(tag: &str) -> (AppState, PathBuf) {
        let dir = temp_dir(tag);
        let db = dir.join("test.db");
        let _ = std::fs::remove_file(&db);
        let store = crate::store::Store::open(&db).expect("打开测试文件库");
        (std::sync::Arc::new(crate::app_state::AppStateInner::with_db(store)), dir)
    }

    #[test]
    fn clear_empties_history_in_the_db_too() {
        let (state, dir) = file_state("clear");

        state
            .store
            .save_playback_position("A", 1, &at(10.0, 100))
            .unwrap();
        assert!(
            !state.store.playback_latest_per_series().unwrap().is_empty(),
            "先确认记录真的写进去了，否则后面的断言没有对照"
        );

        {
            // 模拟「重启」：换一个指向同一文件的全新 Store 实例读
            let reopened =
                crate::store::Store::open(dir.join("test.db")).expect("重开测试库");
            assert!(
                !reopened.playback_latest_per_series().unwrap().is_empty(),
                "落库的数据换个连接也要读得到"
            );
        }

        clear(&state).expect("清空历史不该失败");

        assert!(
            state.store.playback_latest_per_series().unwrap().is_empty(),
            "清空后不该再有历史"
        );
        let reopened = crate::store::Store::open(dir.join("test.db")).expect("重开测试库");
        assert!(
            reopened.playback_latest_per_series().unwrap().is_empty(),
            "只清内存的话，重启后历史全回来了"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clearing_empty_history_is_a_no_op() {
        let (state, dir) = file_state("clear-empty");
        clear(&state).expect("空历史重复清空不该报错");
        assert!(state.store.playback_latest_per_series().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_drops_the_whole_series_not_just_the_latest_episode() {
        let (state, dir) = file_state("remove-one");

        for idx in [1u32, 5, 9] {
            state
                .store
                .save_playback_position("A", idx, &at(1.0, idx as i64))
                .unwrap();
        }
        state
            .store
            .save_playback_position("B", 1, &at(1.0, 1))
            .unwrap();

        remove(&state, "A").unwrap();

        assert!(
            state.store.playback_position("A", 1).unwrap().is_none(),
            "整部剧都要清掉，否则下一集会顶上来，用户以为没删"
        );
        assert!(
            state.store.playback_position("B", 1).unwrap().is_some(),
            "别的剧不受影响"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_reports_not_found_for_unknown_series() {
        let (state, dir) = file_state("remove-missing");
        assert!(remove(&state, "ZZZ").is_err(), "没有记录就不能假装删成功");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_carries_title_and_cover_even_when_series_is_dismissed() {
        let (state, dir) = file_state("history-title");
        state
            .store
            .upsert_series(&Series {
                series_id: "A".into(),
                title: "剧名".into(),
                cover: "封面".into(),
                dismissed: true,
                ..Default::default()
            })
            .unwrap();
        state
            .store
            .save_playback_position("A", 1, &at(10.0, 100))
            .unwrap();

        let items = history_with(&state);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "剧名", "档案被移除也不影响历史带出剧名");
        assert_eq!(items[0].cover, "封面");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_without_a_registry_entry_keeps_the_item_with_blank_title() {
        let (state, dir) = file_state("history-blank");
        state
            .store
            .save_playback_position("B", 1, &at(10.0, 100))
            .unwrap();
        let items = history_with(&state);
        assert_eq!(items.len(), 1, "档案缺失也要显示，只是没剧名没封面");
        assert!(items[0].title.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_picks_the_most_recent_episode_and_sorts_by_recency() {
        let (state, dir) = file_state("history-order");
        // A 有三集，最新是第 7 集；B 比 C 新、比 A 旧
        state
            .store
            .save_playback_position("A", 1, &at(10.0, 100))
            .unwrap();
        state
            .store
            .save_playback_position("A", 7, &at(70.0, 900))
            .unwrap();
        state
            .store
            .save_playback_position("A", 3, &at(30.0, 500))
            .unwrap();
        state
            .store
            .save_playback_position("B", 2, &at(20.0, 800))
            .unwrap();
        state
            .store
            .save_playback_position("C", 3, &at(30.0, 300))
            .unwrap();

        let items = history_with(&state);
        let ids: Vec<_> = items.iter().map(|i| i.series_id.as_str()).collect();
        assert_eq!(ids, ["A", "B", "C"], "按最近观看倒序");
        assert_eq!(items[0].vid_index, 7, "一部剧只显示最新那一集");
        assert_eq!(items[0].current_time, 70.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `history()` 要 `State<'_, AppState>`，测试里用 tauri 的 State 不现实，
    /// 这里复制其两步逻辑（聚合行 + 档案关联）做同等断言。
    fn history_with(state: &AppState) -> Vec<crate::domain::model::PlaybackHistoryItem> {
        use crate::store::entity::LatestPlayback;
        let latest: Vec<LatestPlayback> = state.store.playback_latest_per_series().unwrap();
        let titles: std::collections::HashMap<String, (String, String)> = state
            .store
            .series_all()
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.series_id, (s.title, s.cover)))
            .collect();
        latest
            .into_iter()
            .map(|row| {
                let (title, cover) = titles
                    .get(&row.series_id)
                    .cloned()
                    .unwrap_or((String::new(), String::new()));
                crate::domain::model::PlaybackHistoryItem {
                    series_id: row.series_id,
                    vid_index: row.vid_index,
                    current_time: row.position.current_time,
                    updated_at: row.position.updated_at,
                    title,
                    cover,
                }
            })
            .collect()
    }
}
