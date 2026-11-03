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
/// 两个壳都必须从同一入口调度：在线流预取解决"下一集冷走取流表"，
/// 兼容预转解决"无 HEVC 机器下一集要整集转码"。只调其中一条会让某类
/// 设备继续在自动连播时黑屏等待。
///
/// 两路档位不同：在线流按 `prefetch_episodes` 全量铺，兼容预转只铺
/// [`COMPAT_PREFETCH_LIMIT`] 集——前者只填开头一段（正在播的那一集才填整集），
/// 后者是整集转码。
///
/// 在线预取**逐集串行 + 随机 1~5s 间隔**（2026-11-03：取流域按调用次数
/// 节流，N 集同时发的请求密度会触发按设备限速；逐集 + 随机间隔对齐
/// 官方「每集只发 1 次调用」的自然节奏）。兼容预转只有 1 集，无需间隔。
pub fn schedule_following_episodes(state: &AppState, series_id: &str, current_vid_index: u32) {
    let count = state.settings().prefetch_episodes;
    // 在线预取串行化：一个 spawned task 逐集发，集间随机 1~5s
    let online_state = state.clone();
    let sid = series_id.to_string();
    crate::runtime::spawn(async move {
        for offset in online_offsets(count) {
            online::schedule_next_episode_prefetch(
                online_state.clone(),
                &sid,
                current_vid_index + offset,
            );
            if offset < count {
                // 伪随机 1~5s：纳秒取模足够
                let ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                let delay = 1000 + (ns % 4000) as u64;
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
        }
    });
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

/// 播放策略轮询（行为画像对齐，2026-11-03 官方 74232 实流）：播放器栈
/// 播放期每 `query_gap` 秒（响应下发，默认 300s）拉一次
/// `/reading/media/play/strategy/`。**只取不施策**——官方的默认倍速/
/// 高峰降档/分辨率干预不落地（用户明确不对齐这两项行为）。
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
