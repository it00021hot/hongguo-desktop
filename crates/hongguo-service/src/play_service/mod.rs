//! 播放服务：本地播放、在线播放、断点续播。

use crate::app_state::AppState;

pub mod compat_play;
pub mod compat_prefetch;
pub mod local;
pub mod online;
pub mod position;
pub mod resume;

/// 兼容预转的硬上限。兼容预转是「整集下载 + 整集转码」，不能跟在线流预取
/// 档位线性放大：无 HEVC 的旧机把预取拉到 10 时，10 集同时转码会把 CPU/磁盘
/// 吃满，反而拖慢当前集播放。只保下一集，够连播兜底；再往后按需转。
const COMPAT_PREFETCH_LIMIT: u32 = 1;

/// 当前在线集起播后，按设置向后铺同一部剧的后续集。
///
/// 两个壳都必须从同一入口调度：在线流预取解决“下一集冷走取流表”，
/// 兼容预转解决“无 HEVC 机器下一集要整集转码”。只调其中一条会让某类
/// 设备继续在自动连播时黑屏等待。
///
/// 两路档位不同：在线流按 `prefetch_episodes` 全量铺，兼容预转只铺
/// [`COMPAT_PREFETCH_LIMIT`] 集——前者只填开头一段（正在播的那一集才填整集），
/// 后者是整集转码。
pub fn schedule_following_episodes(state: &AppState, series_id: &str, current_vid_index: u32) {
    let count = state.settings().prefetch_episodes;
    for offset in online_offsets(count) {
        online::schedule_next_episode_prefetch(
            state.clone(),
            series_id,
            current_vid_index + offset,
        );
    }
    for offset in compat_offsets(count) {
        compat_prefetch::schedule_next_episode(
            state.clone(),
            series_id,
            current_vid_index + offset,
        );
    }
}

fn online_offsets(count: u32) -> std::ops::RangeInclusive<u32> {
    1..=count
}

fn compat_offsets(count: u32) -> std::ops::RangeInclusive<u32> {
    1..=count.min(COMPAT_PREFETCH_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_prefetch_does_not_scale_with_the_stream_budget() {
        assert_eq!(
            online_offsets(10).collect::<Vec<_>>(),
            (1..=10).collect::<Vec<_>>()
        );
        assert_eq!(compat_offsets(10).collect::<Vec<_>>(), vec![1]);
        assert_eq!(compat_offsets(0).collect::<Vec<_>>(), Vec::<u32>::new());
        assert_eq!(compat_offsets(1).collect::<Vec<_>>(), vec![1]);
    }
}
