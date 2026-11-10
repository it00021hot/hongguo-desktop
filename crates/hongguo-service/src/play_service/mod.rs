//! 播放服务：本地播放、在线播放、断点续播。

use crate::app_state::AppState;

pub mod compat_play;
pub mod compat_prefetch;
pub mod local;
pub mod online;
pub mod position;
pub mod resume;

/// 兼容预转的硬上限：兼容预转是「整集下载 + 整集转码」，不能跟在线流预取档位线性放大
/// （无 HEVC 的旧机多集同时转码会吃满 CPU/磁盘，反而拖慢当前集）。只保下一集，够连播兜底。
const COMPAT_PREFETCH_LIMIT: u32 = 1;
/// 7.4.3.32 的常态后续模型窗口。
const OFFICIAL_PRELOAD_LIMIT: u32 = 3;

/// 当前在线集起播后，按设置向后铺同一部剧的后续集。两个壳都必须从同一入口调度：
/// 在线预取解决「下一集冷走取流表」、兼容预转解决「无 HEVC 机器下一集要整集转码」，
/// 只调其中一路会让某类设备自动连播黑屏。在线预取**逐集串行 + 随机 1~5s 间隔**
///（取流域按调用次数节流，N 集同时发会触发按设备限速）；兼容预转只铺 `COMPAT_PREFETCH_LIMIT` 集。
pub fn schedule_following_episodes(state: &AppState, series_id: &str, current_vid_index: u32) {
    let v5 = hongguo_api::metasec::playback_configured().unwrap_or(false);
    let mut count = state.settings().prefetch_episodes;
    if v5 {
        count = count.min(OFFICIAL_PRELOAD_LIMIT);
    }
    if count == 0 {
        return;
    }
    let online_state = state.clone();
    let sid = series_id.to_string();
    crate::runtime::spawn(async move {
        if v5 {
            local::prefetch_episode_models(
                &online_state,
                &sid,
                current_vid_index.saturating_add(1)..=current_vid_index.saturating_add(count),
                hongguo_api::params::PlayerModelScene::Player,
            )
            .await;
        }
        for offset in compat_offsets(count) {
            compat_prefetch::schedule_next_episode(
                online_state.clone(),
                &sid,
                current_vid_index + offset,
            );
        }
        for offset in online_offsets(count) {
            online::schedule_next_episode_prefetch(
                online_state.clone(),
                &sid,
                current_vid_index + offset,
            );
            if offset < count {
                let ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                // 预取错峰：1~5s 抖动，避免与起播抢带宽
                const PREFETCH_DELAY_MIN_MS: u64 = 1000;
                const PREFETCH_DELAY_SPAN_MS: u64 = 4000;
                let delay = PREFETCH_DELAY_MIN_MS + (ns as u64 % PREFETCH_DELAY_SPAN_MS);
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
        }
    });
}

fn online_offsets(count: u32) -> std::ops::RangeInclusive<u32> {
    1..=count
}

fn compat_offsets(count: u32) -> std::ops::RangeInclusive<u32> {
    1..=count.min(COMPAT_PREFETCH_LIMIT)
}

/// 播放策略轮询（行为画像对齐）：播放器栈播放期每 `query_gap` 秒（响应下发，默认 300s）
/// 拉一次播放策略接口。**只取不施策**——官方的默认倍速/高峰降档/分辨率干预不落地；
/// 首次在线播放触发，进程内一次；失败按既有节拍顺延。
pub fn ensure_play_strategy_poller(state: &AppState) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let state = state.clone();
    crate::runtime::spawn(async move {
        let mut gap = hongguo_api::play_strategy::DEFAULT_QUERY_GAP_SECS;
        loop {
            let env = state.api_env();
            match hongguo_api::play_strategy::fetch_play_strategy(&env).await {
                Ok(g) => {
                    gap = g;
                    log::info!("[PlayStrategy] 播放策略拉取成功，轮询间隔 {gap}s");
                }
                Err(e) => {
                    log::warn!("[PlayStrategy] 拉取失败（{e}），按 {gap}s 节拍顺延");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(gap)).await;
        }
    });
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
