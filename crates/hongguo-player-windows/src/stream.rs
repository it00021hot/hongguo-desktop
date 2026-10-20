//! MediaEngine 供数：把一集的明文按 offset/length 喂给 Media Foundation。
//!
//! 数据源固定是 `hongguo-stream` 的 `(vid, 档位)` 渐进流（与 macOS 的
//! `av_presenter/loader.rs` 同一段供数语义，只是宿主接口换成了 `IMFByteStream`）：
//! 按需等待密文区间、惰性解密；`GetLength` 用 `plain_len`。
//!
//! spike 实测的两个硬约束（见 `docs/gpui-system-player-windows.md`）：
//! 1. MediaEngine 的供数**只走异步接口**：`BeginRead` 返回 `E_NOTIMPL` 会让它
//!    静默停在「取到长度」之后，一个字节都不读、一个事件都不发；
//! 2. 允许在 `BeginRead` 内同步完成：读到位 → 建 `IMFAsyncResult` → 立刻
//!    `MFInvokeCallback`，`EndRead` 再取字节数。读本身是阻塞的（与 macOS
//!    loader 同一套等待/重试），跑在 MF 自己的工作线程上。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hongguo_stream::{StreamReadError, read_range_cancellable, total_len};
use windows::Win32::Foundation::{E_FAIL, E_NOTIMPL};
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncResult, IMFByteStream, IMFByteStream_Impl, MFBYTESTREAM_IS_READABLE,
    MFBYTESTREAM_IS_SEEKABLE, MFBYTESTREAM_SEEK_ORIGIN, MFCreateAsyncResult, MFInvokeCallback,
    msoCurrent,
};
use windows::core::{Interface, Result, implement};

use hongguo_player_api::StallHook;

/// `msoEnd`（从末尾算的 seek 基准）：windows crate 的 MediaFoundation 模块
/// 只导出了 `msoBegin`/`msoCurrent`，末尾基准按 MF 文档的取值自行构造。
const MFBYTESTREAM_SEEK_END: MFBYTESTREAM_SEEK_ORIGIN = MFBYTESTREAM_SEEK_ORIGIN(2);

/// 一次 `read_range_cancellable` 最多取的字节数（渐进流可能只到了头部）。
const READ_CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// 渐进流注册（`plain_len` 可见）前等长度的上限（与 macOS loader 同参）。
const TOTAL_LEN_TIMEOUT: Duration = Duration::from_secs(5);
/// 单次 `Read` 的总等待上限：超过就当供数失败，让引擎走失败/重试路径。
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// 可恢复读取错误的退避重试次数（8MB 预算耗尽要靠 `on_stall` 续填）。
const STALL_RETRIES: u8 = 3;

struct Inner {
    vid: String,
    definition: u32,
    /// 读前/读失败时的业务续填（`resume_fill`）。
    on_stall: Option<StallHook>,
    position: AtomicU64,
    /// 明文总长；0 = 还没解析出来。
    total: AtomicU64,
    closed: AtomicBool,
    /// `BeginRead` 同步完成后待 `EndRead` 取走的字节数，键是 `IMFAsyncResult` 地址。
    pending: Mutex<Vec<(usize, u32)>>,
}

/// 打开一集的字节流（引擎创建时 `SetSourceFromByteStream` 用）。
pub(super) fn open(vid: String, definition: u32, on_stall: Option<StallHook>) -> IMFByteStream {
    ByteStream {
        inner: Arc::new(Inner {
            vid,
            definition,
            on_stall,
            position: AtomicU64::new(0),
            total: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
        }),
    }
    .into()
}

#[implement(IMFByteStream)]
struct ByteStream {
    inner: Arc<Inner>,
}

fn not_implemented() -> windows::core::Error {
    windows::core::Error::from_hresult(E_NOTIMPL)
}

fn failed(message: &str) -> windows::core::Error {
    log::error!("[WmPresenter] 供数失败：{message}");
    windows::core::Error::from_hresult(E_FAIL)
}

impl Inner {
    /// 等这一档的明文总长（渐进流注册后立即可得）。
    fn total(&self) -> Result<u64> {
        let cached = self.total.load(Ordering::SeqCst);
        if cached != 0 {
            return Ok(cached);
        }
        let deadline = Instant::now() + TOTAL_LEN_TIMEOUT;
        loop {
            if let Some(total) = total_len(&self.vid, self.definition) {
                self.total.store(total, Ordering::SeqCst);
                return Ok(total);
            }
            if self.closed.load(Ordering::SeqCst) {
                return Err(failed("字节流已关闭"));
            }
            if Instant::now() >= deadline {
                return Err(failed("流总长不可用"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// 从当前游标读满 `target`（或到末尾），返回实际字节数。
    fn read_into(&self, target: &mut [u8]) -> Result<u32> {
        if target.is_empty() {
            return Ok(0);
        }
        let position = self.position.load(Ordering::SeqCst);
        let deadline = Instant::now() + READ_TIMEOUT;
        let mut served = 0usize;
        while served < target.len() {
            if self.closed.load(Ordering::SeqCst) {
                return Err(failed("字节流已关闭"));
            }
            if Instant::now() >= deadline {
                return Err(failed("读取超时"));
            }
            let want = (target.len() - served).min(READ_CHUNK_BYTES) as u64;
            match read_with_stall(
                &self.vid,
                self.definition,
                position + served as u64,
                want,
                &self.closed,
                self.on_stall.as_ref(),
            ) {
                Ok(chunk) if !chunk.bytes.is_empty() => {
                    let take = chunk.bytes.len().min(target.len() - served);
                    target[served..served + take].copy_from_slice(&chunk.bytes[..take]);
                    served += take;
                }
                // 空段 = 已到明文末尾（或填充尚未推进）：按已服务的字节返回。
                Ok(_) => break,
                Err(StreamReadError::Cancelled) => return Err(failed("读取被取消")),
                Err(error) => return Err(failed(&error.to_string())),
            }
        }
        self.position
            .store(position + served as u64, Ordering::SeqCst);
        Ok(served as u32)
    }
}

/// 读一块；读前先唤醒业务续填，可恢复错误退避重试（8MB 预算耗尽后只有
/// `resume_fill` 能把填充前沿往前推）。
fn read_with_stall(
    vid: &str,
    definition: u32,
    start: u64,
    want: u64,
    closed: &AtomicBool,
    on_stall: Option<&StallHook>,
) -> std::result::Result<hongguo_stream::StreamChunk, StreamReadError> {
    for attempt in 0..=STALL_RETRIES {
        if let Some(on_stall) = on_stall {
            on_stall();
        }
        let result = read_range_cancellable(vid, definition, start, Some(want), || {
            closed.load(Ordering::SeqCst)
        });
        match result {
            Err(error)
                if attempt < STALL_RETRIES
                    && matches!(
                        &error,
                        StreamReadError::Timeout
                            | StreamReadError::Missing { .. }
                            | StreamReadError::Replaced
                    ) =>
            {
                log::warn!(
                    "[WmPresenter] {vid}@{definition} 读取 {start}..{} 暂时失败（{error}），{}ms 后重试（{}/{STALL_RETRIES}）",
                    start + want,
                    250u64 * (u64::from(attempt) + 1),
                    attempt + 1
                );
                std::thread::sleep(Duration::from_millis(250 * (u64::from(attempt) + 1)));
            }
            other => return other,
        }
    }
    unreachable!("STALL_RETRIES 范围内必返回")
}

impl IMFByteStream_Impl for ByteStream_Impl {
    fn GetCapabilities(&self) -> Result<u32> {
        Ok(MFBYTESTREAM_IS_READABLE | MFBYTESTREAM_IS_SEEKABLE)
    }

    fn GetLength(&self) -> Result<u64> {
        self.inner.total()
    }

    fn SetLength(&self, _length: u64) -> Result<()> {
        Err(not_implemented())
    }

    fn GetCurrentPosition(&self) -> Result<u64> {
        Ok(self.inner.position.load(Ordering::SeqCst))
    }

    fn SetCurrentPosition(&self, position: u64) -> Result<()> {
        self.inner.position.store(position, Ordering::SeqCst);
        Ok(())
    }

    fn IsEndOfStream(&self) -> Result<windows::core::BOOL> {
        let position = self.inner.position.load(Ordering::SeqCst);
        let total = self.inner.total.load(Ordering::SeqCst);
        Ok(windows::core::BOOL::from(total != 0 && position >= total))
    }

    fn Read(&self, buffer: *mut u8, length: u32, read: *mut u32) -> Result<()> {
        if length == 0 {
            unsafe { *read = 0 };
            return Ok(());
        }
        let target = unsafe { std::slice::from_raw_parts_mut(buffer, length as usize) };
        let served = self.inner.read_into(target)?;
        unsafe { *read = served };
        Ok(())
    }

    fn BeginRead(
        &self,
        buffer: *mut u8,
        length: u32,
        callback: windows::core::Ref<IMFAsyncCallback>,
        state: windows::core::Ref<windows::core::IUnknown>,
    ) -> Result<()> {
        // 同步读到位再触发回调：MF 允许 Begin 内完成，而 MediaEngine 只走这条
        // 异步入口（spike 实测：只实现同步 Read 会被静默忽略）。
        let mut served = 0u32;
        IMFByteStream_Impl::Read(self, buffer, length, &mut served)?;
        let result = unsafe { MFCreateAsyncResult(None, callback.as_ref(), state.as_ref())? };
        let key = result.as_raw() as usize;
        self.inner
            .pending
            .lock()
            .expect("字节流 pending 锁 poisoned")
            .push((key, served));
        unsafe { MFInvokeCallback(&result)? };
        Ok(())
    }

    fn EndRead(&self, result: windows::core::Ref<IMFAsyncResult>) -> Result<u32> {
        let key = result.as_ref().map_or(0, |result| result.as_raw() as usize);
        let mut pending = self
            .inner
            .pending
            .lock()
            .expect("字节流 pending 锁 poisoned");
        let index = pending.iter().position(|(k, _)| *k == key);
        Ok(index.map(|i| pending.remove(i).1).unwrap_or(0))
    }

    fn Write(&self, _buffer: *const u8, _length: u32) -> Result<u32> {
        Err(not_implemented())
    }

    fn BeginWrite(
        &self,
        _buffer: *const u8,
        _length: u32,
        _callback: windows::core::Ref<IMFAsyncCallback>,
        _state: windows::core::Ref<windows::core::IUnknown>,
    ) -> Result<()> {
        Err(not_implemented())
    }

    fn EndWrite(&self, _result: windows::core::Ref<IMFAsyncResult>) -> Result<u32> {
        Err(not_implemented())
    }

    fn Seek(&self, origin: MFBYTESTREAM_SEEK_ORIGIN, offset: i64, _flags: u32) -> Result<u64> {
        // 总长走 `total()` 而不是裸原子：裸原子在 `GetLength` 之前是 0，
        // 拿它当上界会把任何 seek 钳回 0（拖进度条后画面回到开头）。
        let total = self.inner.total()? as i64;
        let base = if origin == msoCurrent {
            self.inner.position.load(Ordering::SeqCst) as i64
        } else if origin == MFBYTESTREAM_SEEK_END {
            total
        } else {
            0
        };
        let next = (base + offset).clamp(0, total.max(0)) as u64;
        self.inner.position.store(next, Ordering::SeqCst);
        Ok(next)
    }

    fn Flush(&self) -> Result<()> {
        Ok(())
    }

    fn Close(&self) -> Result<()> {
        self.inner.closed.store(true, Ordering::SeqCst);
        Ok(())
    }
}
