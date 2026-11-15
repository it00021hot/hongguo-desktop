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
