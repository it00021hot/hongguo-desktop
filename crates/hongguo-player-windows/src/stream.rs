//! MediaEngine 供数：把一集的明文按 offset/length 喂给 Media Foundation。系统播放器的数据源是 `hongguo-stream` 的 `(vid, 档位)` 渐进流；
//! 内置软解直播放另有本地文件来源与「只剩音轨」视图（[`Feed`] / [`AudioMask`]）。渐进流的供数（与 macOS 的 `av_presenter/loader.rs` 同一段供数语义，只是宿主接口换成 `IMFByteStream`）：按需等待密文区间、惰性解密，`GetLength` 用 `plain_len`。
//! spike 实测的两个硬约束（见 `docs/gpui-system-player-windows.md`）：① MediaEngine 供数**只走异步接口**——`BeginRead` 返回 `E_NOTIMPL` 会让它
//! 静默停在「取到长度」之后，一个字节都不读、一个事件都不发；② 允许在 `BeginRead` 内同步完成（读到位 → 建 `IMFAsyncResult` → 立刻 `MFInvokeCallback`，
//! `EndRead` 再取字节数）——读本身阻塞（与 macOS loader 同一套等待/重试），跑在 MF 自己的工作线程上。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use hongguo_softdec::IndexedVideo;
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
/// 可恢复读取错误的退避重试次数（头部预算耗尽要靠 `on_stall` 续填）。
const STALL_RETRIES: u8 = 3;

/// 字节从哪里来：在线流（渐进流的明文 Range）或本地文件。系统播放器只用在线流
/// （本地文件直接交 URL 给引擎）；内置软解直播放两种都要经本字节流，才能遮掉视频轨。
#[derive(Clone)]
pub(super) enum Feed {
    Online {
        vid: String,
        definition: u32,
        /// 读前/读失败时的业务续填（`resume_fill`）。
        on_stall: Option<StallHook>,
    },
    File(PathBuf),
}

impl Feed {
    pub(super) fn label(&self) -> String {
        match self {
            Feed::Online {
                vid, definition, ..
            } => format!("{vid}@{definition}"),
            Feed::File(path) => path.display().to_string(),
        }
    }
}

/// 「只剩音轨」视图：索引由起播线程建好后放进来，读到的字节按它遮掉视频轨。
/// 索引就绪前引擎的读会等着（读本来就会阻塞，跑在 MF 自己的工作线程上）。
#[derive(Default)]
pub(super) struct AudioMask {
    index: OnceLock<Arc<IndexedVideo>>,
    /// 索引建不起来：在等的读立即失败，引擎走失败路径。
    failed: AtomicBool,
}

impl AudioMask {
    pub(super) fn ready(&self, index: Arc<IndexedVideo>) {
        let _ = self.index.set(index);
    }

    pub(super) fn fail(&self) {
        self.failed.store(true, Ordering::SeqCst);
    }

    fn wait(&self, closed: &AtomicBool) -> Result<&IndexedVideo> {
        let deadline = Instant::now() + READ_TIMEOUT;
        loop {
            if let Some(index) = self.index.get() {
                return Ok(index);
            }
            if self.failed.load(Ordering::SeqCst) {
                return Err(failed("音轨视图的索引没建起来"));
            }
            if closed.load(Ordering::SeqCst) {
                return Err(failed("字节流已关闭"));
            }
            if Instant::now() >= deadline {
                return Err(failed("等音轨视图的索引超时"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct Inner {
    feed: Feed,
    /// 本地文件句柄（首次读时打开；一集上千次读，逐次开文件是纯开销）。
    file: Mutex<Option<File>>,
    /// 设了就是「只剩音轨」视图。
    mask: Option<Arc<AudioMask>>,
    position: AtomicU64,
    /// 明文总长；0 = 还没解析出来。
    total: AtomicU64,
    closed: AtomicBool,
    /// `BeginRead` 同步完成后待 `EndRead` 取走的字节数，键是 `IMFAsyncResult` 地址。
    pending: Mutex<Vec<(usize, u32)>>,
}

/// 打开一集的字节流（引擎创建时 `SetSourceFromByteStream` 用）。
pub(super) fn open(vid: String, definition: u32, on_stall: Option<StallHook>) -> IMFByteStream {
    open_feed(
        Feed::Online {
            vid,
            definition,
            on_stall,
        },
        None,
    )
}

/// 打开任意来源的字节流；`mask` 给了就是「只剩音轨」视图（内置软解直播放用）。
pub(super) fn open_feed(feed: Feed, mask: Option<Arc<AudioMask>>) -> IMFByteStream {
    ByteStream {
        inner: Arc::new(Inner {
            feed,
            file: Mutex::new(None),
            mask,
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
        let (vid, definition) = match &self.feed {
            Feed::Online {
                vid, definition, ..
            } => (vid, *definition),
            Feed::File(path) => {
                let total = std::fs::metadata(path)
                    .map_err(|error| failed(&format!("读不到本地文件长度: {error}")))?
                    .len();
                self.total.store(total, Ordering::SeqCst);
                return Ok(total);
            }
        };
        let deadline = Instant::now() + TOTAL_LEN_TIMEOUT;
        loop {
            if let Some(total) = total_len(vid, definition) {
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
        let mask = match &self.mask {
            Some(mask) => Some(mask.wait(&self.closed)?),
            None => None,
        };
        let position = self.position.load(Ordering::SeqCst);
        let served = match &self.feed {
            Feed::Online {
                vid,
                definition,
                on_stall,
            } => read_online(
                vid,
                *definition,
                position,
                target,
                &self.closed,
                on_stall.as_ref(),
            )?,
            Feed::File(path) => {
                let mut file = self.file.lock().expect("本地文件锁 poisoned");
                if file.is_none() {
                    *file = Some(
                        File::open(path)
                            .map_err(|error| failed(&format!("打不开本地文件: {error}")))?,
                    );
                }
                let file = file.as_mut().expect("刚打开");
                read_file(file, position, target).map_err(|error| failed(&error.to_string()))?
            }
        };
        if let Some(index) = mask {
            index.mask_video_tracks(position, &mut target[..served]);
        }
        self.position
            .store(position + served as u64, Ordering::SeqCst);
        Ok(served as u32)
    }
}

/// 从本地文件 `position` 起读满 `target`（或到文件尾），返回实际字节数。
fn read_file(file: &mut File, position: u64, target: &mut [u8]) -> std::io::Result<usize> {
    file.seek(SeekFrom::Start(position))?;
    let mut served = 0;
    while served < target.len() {
        match file.read(&mut target[served..])? {
            0 => break,
            read => served += read,
        }
    }
    Ok(served)
}

/// 从在线流 `position` 起读满 `target`（或到明文末尾），返回实际字节数。
fn read_online(
    vid: &str,
    definition: u32,
    position: u64,
    target: &mut [u8],
    closed: &AtomicBool,
    on_stall: Option<&StallHook>,
) -> Result<usize> {
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut served = 0usize;
    while served < target.len() {
        if closed.load(Ordering::SeqCst) {
            return Err(failed("字节流已关闭"));
        }
        if Instant::now() >= deadline {
            return Err(failed("读取超时"));
        }
        let want = (target.len() - served).min(READ_CHUNK_BYTES) as u64;
        match read_with_stall(
            vid,
            definition,
            position + served as u64,
            want,
            closed,
            on_stall,
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
    Ok(served)
}

/// 解码线程按 (offset, length) 读原始字节（不遮视频轨）：读不满就是错误。
/// 本地文件句柄由调用方持有（一个解码会话开一次）。
pub(super) fn read_exact(
    feed: &Feed,
    file: &mut Option<File>,
    offset: u64,
    length: u64,
    cancelled: &AtomicBool,
    bytes: &mut Vec<u8>,
) -> std::result::Result<(), String> {
    let length = usize::try_from(length).map_err(|_| "读取范围过大".to_string())?;
    bytes.resize(length, 0);
    let served = match feed {
        Feed::Online {
            vid,
            definition,
            on_stall,
        } => read_online(
            vid,
            *definition,
            offset,
            bytes,
            cancelled,
            on_stall.as_ref(),
        )
        .map_err(|error| error.to_string())?,
        Feed::File(path) => {
            if file.is_none() {
                *file = Some(File::open(path).map_err(|error| error.to_string())?);
            }
            read_file(file.as_mut().expect("刚打开"), offset, bytes)
                .map_err(|error| error.to_string())?
        }
    };
    if served == length {
        Ok(())
    } else {
        Err(format!("{offset} 起只读到 {served}/{length} 字节"))
    }
}

/// 这一集的总字节数（解码侧建索引用）；拿不到返回 `None`。
pub(super) fn total_bytes(feed: &Feed) -> Option<u64> {
    match feed {
        Feed::File(path) => std::fs::metadata(path).ok().map(|meta| meta.len()),
        Feed::Online {
            vid, definition, ..
        } => {
            let deadline = Instant::now() + TOTAL_LEN_TIMEOUT;
            loop {
                if let Some(total) = total_len(vid, *definition) {
                    return Some(total);
                }
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// 读一块；读前先唤醒业务续填，可恢复错误退避重试（头部预算耗尽后只有
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
