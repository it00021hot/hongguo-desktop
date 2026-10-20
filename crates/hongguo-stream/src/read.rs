//! 字节级读取 API：给 `AVAssetResourceLoader` 这类「要 offset/length、没有
//! HTTP Range 头」的消费者用。
//!
//! 与 [`crate::serve`] 的关系：**同一套等待/解密/ReaderDemand**，只是出口
//! 不同——`serve` 产出 `(状态码, 响应头, 响应体)`，这里产出
//! [`StreamChunk`]。HTTP 头、206/416 是协议壳语义，不属于数据面。
//!
//! 语义约定（AVFoundation 依赖这些约定）：
//! - [`total_len`] 在渐进流注册后立刻返回 `plain_len`，不需要等任何字节；
//! - `len = None` 表示读到明文末尾；`len = Some(n)` 最多读 `n` 字节，
//!   越过末尾自动截断（不报错）；
//! - `start > total` 才是 [`StreamReadError::OutOfRange`]；`start == total`
//!   是合法的 EOF 读，返回空 `bytes`；
//! - 数据未就绪时阻塞等待（最多 30s），`cancelled` 命中或条目被逐出立即
//!   返回错误并注销读者登记。

use std::fmt;
use std::sync::Arc;

use crate::cache::StreamEntry;
use crate::progressive::{EntryMode, read_plain_range, wait_for_entry};

/// 一次字节读取的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamChunk {
    /// 明文总长（与 [`total_len`] 一致）。
    pub total: u64,
    /// 本次读取的起始偏移。
    pub start: u64,
    /// 明文字节；可能短于请求长度（只会在到达末尾时发生）。
    pub bytes: Vec<u8>,
}

/// 字节级读取的错误。`serve()` 会把它折叠成原来的错误字符串，HTTP 语义不变。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamReadError {
    /// 没有 (vid, 档位) 条目：从未起播，或已被逐出。
    Missing { vid: String, definition: u32 },
    /// 等待期间条目被逐出 / 同键重建，手里的填充已经死亡。
    Replaced,
    /// 等待数据就绪超时（填充停摆或网络断供）。
    Timeout,
    /// 调用方取消（seeking / 切集时旧请求退场）。
    Cancelled,
    /// `start` 超出明文总长。
    OutOfRange { start: u64, total: u64 },
}

impl fmt::Display for StreamReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { vid, definition } => {
                write!(formatter, "没有正在准备的在线流: {vid}@{definition}")
            }
            Self::Replaced => formatter.write_str("在线流已被替换，请重试"),
            Self::Timeout => formatter.write_str("等待流就绪超时"),
            Self::Cancelled => formatter.write_str("读取已取消（seek 重定位）"),
            Self::OutOfRange { start, total } => {
                write!(formatter, "读取越界: start={start}, total={total}")
            }
        }
    }
}

impl std::error::Error for StreamReadError {}

/// 这一档的明文总长。渐进流注册后立即可用（`plain_len`）；整集模式要等
/// 数据全部落盘后才有值（`None` 表示还没就绪）。
///
/// AVFoundation 的 `contentInformationRequest.contentLength` 依赖它——必须
/// 在创建 `AVURLAsset` 之前拿到，所以调用方应先等本函数返回 `Some`。
pub fn total_len(vid: &str, definition: u32) -> Option<u64> {
    let entry = crate::cache().get(vid, definition)?;
    if let Some(progressive) = entry.progressive.lock().as_ref() {
        return Some(progressive.plain_len);
    }
    let size = *entry.size.lock();
    (size > 0).then_some(size)
}

/// 读一段明文。数据未就绪时阻塞等待（最多 30s）。
///
/// 不关心取消语义的调用方用这个；播放器的 seek / 切集路径应使用
/// [`read_range_cancellable`]，否则旧位置的读取会拖住填充调度。
pub fn read_range(
    vid: &str,
    definition: u32,
    start: u64,
    len: Option<u64>,
) -> Result<StreamChunk, StreamReadError> {
    read_range_inner(vid, definition, start, len, &|| false)
}

/// [`read_range`] 的可取消变体：等待期间逐片检查 `cancelled`，命中即注销
/// 读者登记并立即返回 [`StreamReadError::Cancelled`]。
pub fn read_range_cancellable(
    vid: &str,
    definition: u32,
    start: u64,
    len: Option<u64>,
    cancelled: impl Fn() -> bool,
) -> Result<StreamChunk, StreamReadError> {
    read_range_inner(vid, definition, start, len, &cancelled)
}

fn read_range_inner(
    vid: &str,
    definition: u32,
    start: u64,
    len: Option<u64>,
    cancelled: &dyn Fn() -> bool,
) -> Result<StreamChunk, StreamReadError> {
    let (lease, mode) = wait_for_entry(vid, definition, cancelled)?;
    let total = match &mode {
        EntryMode::Whole => *lease.entry.size.lock(),
        EntryMode::Progressive(progressive) => progressive.plain_len,
    };
    if start > total {
        return Err(StreamReadError::OutOfRange { start, total });
    }
    let end = match len {
        None => total,
        Some(0) => start,
        Some(requested) => start.saturating_add(requested).min(total),
    };
    let bytes = match mode {
        EntryMode::Whole => read_whole(&lease.entry, start, end, total)?,
        EntryMode::Progressive(progressive) => {
            read_plain_range(&progressive, start, end, &lease, cancelled)?
        }
    };
    Ok(StreamChunk {
        total,
        start,
        bytes,
    })
}

/// 整集模式：数据全在内存，直接切。锁顺序与 `StreamCache::store` 一致
/// （buffer → size），避免与写入侧交叉等待。
fn read_whole(
    entry: &Arc<StreamEntry>,
    start: u64,
    end: u64,
    total: u64,
) -> Result<Vec<u8>, StreamReadError> {
    let buffer = entry.buffer.lock();
    let size = *entry.size.lock();
    let start_index = start as usize;
    let end_index = end as usize;
    let Some(slice) = buffer.get(start_index..end_index) else {
        return Err(StreamReadError::OutOfRange { start, total });
    };
    // size 与 buffer 由 store 一次性写入；不一致说明缓存被外部改坏了。
    debug_assert_eq!(
        size,
        buffer.len() as u64,
        "整集条目 size 与 buffer 长度不一致"
    );
    Ok(slice.to_vec())
}
