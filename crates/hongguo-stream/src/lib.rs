//! 在线播放的内存流数据面（hongguo-stream crate）。
//!
//! 缓存按 **(vid, 清晰度档位)** 分条目：一集有几个档位就有几份互不相干的数据。
//! 于是切清晰度**不用碰上一档的任何字节**——正在播的旧 `<video>` 继续从
//! 自己那份 buffer 取数，直到它被换掉为止。
//!
//! ⚠️ 早期版本只按 vid 分条目，切档必须原地清空 buffer，再靠「代次号」去识别
//!    上一代遗留的 Range 请求。被清空的那份恰好是还在播的那份，`<video>` 下一发
//!    请求要么被拒、要么切出错位片段，立刻 `onError` —— 表现为「视频处理失败」
//!    黑屏。分条目之后这套机制没有存在意义，整块拿掉。
//!
//! 两条供数路径，谁先就绪走谁：
//! - **渐进**：注册了 [`ProgressiveStream`]（稀疏密文 + 解密计划）后即可应答，
//!   按明文 Range 等齐依赖的密文区间、惰性解密拼装（见 [`serve`]）。
//!   首帧只需头 + 尾 + moov 就位。
//! - **整集**：明文一次性原子写进 [`StreamCache`]，`size` 从 0 变成真实值的那一刻起
//!   全部字节就都在内存里，任意 Range 直接切。渐进路径不可用（CDN 不支持
//!   Range 等）时的回落。
//!
//! 渐进模式的开放式 Range 必须开窗口（默认 4MB，`HONGGUO_STREAM_WINDOW`
//! 可覆盖）：给到底就等于要等整集，渐进失去意义。整集模式保持「一次给到底」。
//!
//! **字节级读取**（[`read_range`] / [`read_range_cancellable`] / [`total_len`]）
//! 与 HTTP 版 [`serve`] 共用同一套等待/解密/读者需求登记，供
//! `AVAssetResourceLoader` 这类只要 offset/length 的消费者使用。
//!
//! ## 为什么独立成 crate
//!
//! 本 crate 是两端共用的**上游**：
//!
//! - 填充侧——`hongguo-service` 的 play_service 解析出流地址后往里推数据；
//! - 应答侧——`hongguo-protocol` 的 `hongguo-stream://` 处理器从里读数据、答 Range。
//!
//! 两侧都只往下依赖本 crate，不再互相引用。进程级缓存 [`cache`] 放在这里，
//! 正是为了掐掉「协议层回头找服务层要缓存」那条反向边（`serve` 曾经直接调
//! `service::play_service::online::cache()`，与 service 用本层类型构成双向耦合）。
//!
//! 不依赖 tauri / 协议壳 / 服务层；依赖只有 [`hongguo_video`]（稀疏缓冲与解密计划）
//! 与 `hongguo-common`（档位模型）。

mod cache;
mod progressive;
mod read;

pub mod range;

pub use cache::StreamCache;
pub use progressive::{ProgressiveStream, ReaderDemand, serve, serve_cancellable};
pub use read::{StreamChunk, StreamReadError, read_range, read_range_cancellable, total_len};

use std::sync::OnceLock;

/// 全局流缓存——填充侧与应答侧共用同一份。
///
/// 进程级单例是刻意的：`hongguo-stream://` 的 Range 请求由协议线程处理，
/// 而数据由服务层的后台填充任务推进去，两者之间只有这个句柄可共享。
pub fn cache() -> &'static StreamCache {
    static CACHE: OnceLock<StreamCache> = OnceLock::new();
    CACHE.get_or_init(StreamCache::default)
}
