//! 在线播放的内存流数据面：缓存按 (vid, 清晰度档位) 分条目，渐进（稀疏解密拼装）
//! 与整集（一次性原子写入）两条供数路径，谁先就绪走谁。
//! 填充侧（hongguo-service）与应答侧（hongguo-protocol）共用的上游，不依赖 tauri / 服务层。

mod cache;
mod progressive;
mod read;

pub mod range;

pub use cache::StreamCache;
pub use progressive::{ProgressiveStream, ReaderDemand, serve, serve_cancellable};
pub use read::{StreamChunk, StreamReadError, read_range, read_range_cancellable, total_len};

use std::sync::OnceLock;

/// 全局流缓存——进程级单例：协议线程应答 Range 与服务层后台填充只有这个句柄可共享。
pub fn cache() -> &'static StreamCache {
    static CACHE: OnceLock<StreamCache> = OnceLock::new();
    CACHE.get_or_init(StreamCache::default)
}
