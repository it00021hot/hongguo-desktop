//! 在线播放：取流 → 解密 → 推进内存流。
//!
//! 数据面在 [`hongguo_stream`]，这里只负责把数据推进去。
//!
//! 两条取流路径：
//! - **渐进**（首选）：首探头部 256KB → 盒游走定位 moov → 精确补齐 → 建
//!   解密计划 → 注册进协议层（此刻起即可应答 Range）→ 顺序填充余下字节
//!   （探针序列对齐官方取流 worker）。seek 到未下载区段时由读者需求
//!   登记（ReaderDemand）驱动优先填充，回跳与前进一视同仁。
//! - **整集**（回落）：CDN 不支持 Range、moov 定位失败等情形下，整集取回 +
//!   解密后一次性交给协议，行为与本功能加入前完全一致。
//!
//! ⚠️ 起后台填充任务一律走共用任务运行时（[`crate::runtime`]），不裸
//! `tokio::spawn`：后者依赖调用点所在的 Tokio 上下文，[`resume_fill`] 是
//! 同步入口、可能被协议/回调线程调用（那里没有上下文，会 panic→abort）。
//! 运行时是进程级共享的，任意线程都可安全派发。
//!
//! 本模块壳无关：进度事件发布到 service 事件总线（[`crate::events`]），
//! Tauri / GPUI 各自订阅适配。
//!
//! 子模块：`prepare`（取流准备与预取编排）、`fill`（填充门闸与填充循环）、
//! `plain`（整集抓取）、`progress`（进度设施）；公开面经本模块 re-export。

mod fill;
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
