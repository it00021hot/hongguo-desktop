//! 播放服务：本地播放、在线播放、断点续播。

use crate::app_state::AppState;

pub mod compat_play;
pub mod compat_prefetch;
pub mod local;
pub mod online;
pub mod position;
pub mod resume;

/// 当前在线集起播后，按设置向后铺同一部剧的后续集。
///
/// 两个壳都必须从同一入口调度：在线流预取解决“下一集冷走取流表”，
/// 兼容预转解决“无 HEVC 机器下一集要整集转码”。只调其中一条会让某类
/// 设备继续在自动连播时黑屏等待。
pub fn schedule_following_episodes(state: &AppState, series_id: &str, current_vid_index: u32) {
    let count = state.settings().prefetch_episodes;
    for offset in 1..=count {
        let next_vid_index = current_vid_index + offset;
        compat_prefetch::schedule_next_episode(state.clone(), series_id, next_vid_index);
        online::schedule_next_episode_prefetch(state.clone(), series_id, next_vid_index);
    }
}
