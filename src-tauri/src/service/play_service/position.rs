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

    /// 断点续播的持久化语义：保存后「重启」（重开同一文件库）还能读回；
    /// 接近片尾的记录视为看完，load 返回 0（从头看）。
    #[test]
    fn save_survives_reopen_and_load_skips_near_end() {
        let (state, dir) = file_state("persist");

        state
            .store
            .save_playback_position("A", 1, &at(10.0, 100))
            .unwrap();
        state
            .store
            .save_playback_position("B", 1, &at(290.0, 100))
            .unwrap();

        {
            // 模拟「重启」：换一个指向同一文件的全新 Store 实例读
            let reopened = crate::store::Store::open(dir.join("test.db")).expect("重开测试库");
            let pos = reopened.playback_position("A", 1).unwrap();
            assert!(pos.is_some(), "落库的进度换个连接也要读得到");
        }

        // load 是 store 直读的一层薄过滤（unwrap + 近片尾归零），这里按
        // store 语义断言，绕开测试里构造不到的 tauri State
        let a = state.store.playback_position("A", 1).unwrap().unwrap();
        assert_eq!(a.current_time, 10.0, "续播位置原样读回");
        assert!(!a.is_near_end());
        let b = state.store.playback_position("B", 1).unwrap().unwrap();
        assert!(b.is_near_end(), "接近片尾视为看完，load 会归零从头播");
        assert!(
            state.store.playback_position("ZZZ", 1).unwrap().is_none(),
            "没记录的剧从头播"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
