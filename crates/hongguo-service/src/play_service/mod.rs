//! 播放服务：本地播放、在线播放、断点续播。

use crate::app_state::AppState;

pub mod compat_play;
mod feed;
pub mod local;
pub mod online;
pub mod position;
mod prefetch;
pub mod resume;

pub use feed::prefetch_feed;
pub use hongguo_api::prefetch_policy::DEFAULT_PREFETCH_COUNT;
pub(crate) use prefetch::PrefetchCache;
pub use prefetch::prefetch_budget;

/// 当前在线集起播后，向后铺同一部剧的后续集。两个壳从同一入口调度：
/// 在线预取按动态字节预算铺媒体，兼容转码由实际选中的分集触发。
pub fn schedule_following_episodes(state: &AppState, series_id: &str, current_vid_index: u32) {
    let v5 = hongguo_api::metasec::playback_configured().unwrap_or(false);
    let online_state = state.clone();
    let sid = series_id.to_string();
    crate::runtime::spawn(async move {
        let budget = prefetch_budget(&online_state).await;
        let count = budget.count;
        if count == 0 {
            return;
        }
        if v5 {
            local::prefetch_episode_models(
                &online_state,
                &sid,
                current_vid_index.saturating_add(1)..=current_vid_index.saturating_add(count),
                hongguo_api::params::PlayerModelScene::Player,
            )
            .await;
        }
        for offset in 1..=count {
            online::schedule_next_episode_prefetch(
                online_state.clone(),
                &sid,
                current_vid_index.saturating_add(offset),
                budget.bytes,
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
