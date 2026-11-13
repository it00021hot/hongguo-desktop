//! 在线播放：取流 → 解密 → 推进内存流（数据面在 [`hongguo_stream`]，这里只负责把数据推进去）。
//! 两条取流路径：**渐进**（首选：首探头部 → 定位 moov → 建解密计划 → 注册协议层 → 顺序填充，
//! seek 由 ReaderDemand 驱动优先填充）与**整集**（回落：整集取回 + 解密一次性交给协议）。
//! 红线：起后台填充任务一律走共用任务运行时 [`crate::runtime`]，不裸 `tokio::spawn`——
//! `resume_fill` 是同步入口、可能被协议/回调线程调用（无 Tokio 上下文，裸 spawn 会 panic→abort）。

mod cached;
pub(crate) mod disk;
mod fill;
pub(in crate::play_service) use cached::plain as cached_plain;
mod plain;
mod prepare;
mod progress;

pub use fill::resume_fill;
pub use plain::{fetch_plain, fetch_plain_with};
pub use prepare::{
    Prepared, ensure_stream_ready, prefetch_stream, prepare, schedule_next_episode_prefetch,
};
pub use progress::{OnlineProgressEvent, ProgressReporter, ProgressSink};

use hongguo_stream::cache;

/// 在线缓存现状（分条数、已填充字节），供缓存占用统计读。
pub fn status() -> (usize, u64) {
    cache().status()
}

/// 清空在线缓存。
pub fn clear() -> usize {
    let (count, _) = cache().status();
    cache().clear();
    count
}
