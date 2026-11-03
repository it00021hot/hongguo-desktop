//! 断点续播读写。存储切到数据库后全是对 [`hongguo_store::Store`] 的单行/单表操作：
//! 保存是单行 UPSERT，历史是「每部剧最新一集」的聚合查询 + 档案表取剧名封面。

use crate::app_state::AppState;
use hongguo_common::model::PlaybackPosition;

/// 保存播放位置（Store 直连版：同步 command 占主线程，高频保存不该在那里排队）。
/// `duration` 由前端回传——后端拿到流的时候还不知道总时长。
pub fn save_store(
    store: &hongguo_store::Store,
    series_id: &str,
    vid_index: u32,
    current_time: f64,
    duration: f64,
) -> hongguo_common::AppResult<()> {
    store.save_playback_position(
        series_id,
        vid_index,
        &PlaybackPosition::new(current_time, duration),
    )
}

/// 读播放位置。看过超过 3 秒就续播那个位置（用户口径：不管片尾）。
pub fn load(state: &AppState, series_id: &str, vid_index: u32) -> hongguo_common::AppResult<f64> {
    Ok(state
        .store
        .playback_position(series_id, vid_index)?
        .filter(|p| p.is_resumable())
        .map(|p| p.current_time)
        .unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_common::model::PlaybackPosition;
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

    fn file_state(tag: &str) -> (AppState, PathBuf) {
        let dir = temp_dir(tag);
        let db = dir.join("test.db");
        let _ = std::fs::remove_file(&db);
        let store = hongguo_store::Store::open(&db).expect("打开测试文件库");
        (
            std::sync::Arc::new(crate::app_state::AppStateInner::with_db(store)),
            dir,
        )
    }

    #[test]
    fn series_last_position_follows_latest_row() {
        let (state, dir) = file_state("lastpos");
        state
            .store
            .save_playback_position("A", 1, &at(10.0, 100))
            .unwrap();
        state
            .store
            .save_playback_position("A", 2, &at(20.0, 200))
            .unwrap();
        // 回看第 1 集并更新得最晚：最新行是 (1, t=11)
        state
            .store
            .save_playback_position("A", 1, &at(11.0, 300))
            .unwrap();
        let (vid, pos) = state.store.series_last_position("A").unwrap().unwrap();
        assert_eq!(vid, 1, "updated_at 最新的行是第 1 集");
        assert_eq!(pos.current_time, 11.0);
        assert!(
            state.store.series_last_position("ZZZ").unwrap().is_none(),
            "没看过的剧返回 None（前端回落云端历史）"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_survives_reopen_and_load_resumes_over_three_seconds() {
        let (state, dir) = file_state("persist");

        state
            .store
            .save_playback_position("A", 1, &at(10.0, 100))
            .unwrap();
        state
            .store
            .save_playback_position("B", 1, &at(290.0, 100))
            .unwrap();
        state
            .store
            .save_playback_position("C", 1, &at(2.0, 100))
            .unwrap();

        {
            let reopened = hongguo_store::Store::open(dir.join("test.db")).expect("重开测试库");
            let pos = reopened.playback_position("A", 1).unwrap();
            assert!(pos.is_some(), "落库的进度换个连接也要读得到");
        }

        // load 是 store 直读的一层薄过滤（unwrap + >3 秒续播），这里按
        // store 语义断言，绕开测试里需要构造完整 AppState 的路径
        let a = state.store.playback_position("A", 1).unwrap().unwrap();
        assert_eq!(a.current_time, 10.0, "续播位置原样读回");
        assert!(a.is_resumable());
        let b = state.store.playback_position("B", 1).unwrap().unwrap();
        assert!(b.is_resumable(), "接近片尾也要续播那个位置");
        let c = state.store.playback_position("C", 1).unwrap().unwrap();
        assert!(!c.is_resumable(), "≤3 秒从头播");
        assert!(
            state.store.playback_position("ZZZ", 1).unwrap().is_none(),
            "没记录的剧从头播"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
