//! 渐进流数据面：`ProgressiveStream` 数据结构 + 协议层 Range 应答 + 窗口策略。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use crate::cache::StreamEntry;
use crate::range::{ProtocolResponse, RangeSpec, parse_range, partial_headers};
use crate::read::StreamReadError;
use hongguo_video::streaming::{SparseBuffer, StreamingPlan};

/// 等待数据就绪的最长时间（秒）。超时返回 502，避免请求永久挂起。
const WAIT_TIMEOUT_SECS: u64 = 30;

/// 渐进式在线流的开放 Range 默认窗口：一次回 4MB。必须开窗口，
/// 否则 `bytes=0-` 要等整集下载完才应答，渐进就没有意义了。
const PROGRESSIVE_WINDOW: u64 = 4 * 1024 * 1024;

/// 渐进式在线流：稀疏密文 + 解密计划，协议层按 Range 惰性解密；`plan` 为 `None` 表示未加密流。
/// `cdn_url`/`proxy` 供预取转正后的恢复填充按 Range 续拉，免去重放取流表那次接口调用。
pub struct ProgressiveStream {
    pub sparse: Arc<SparseBuffer>,
    pub plan: Option<Arc<StreamingPlan>>,
    /// 明文总长（`plan` 为 `None` 时等于密文总长）
    pub plain_len: u64,
    /// 等数据的读者登记表：填充调度器据此决定下一块下哪里（见 [`ReaderDemand`]）
    pub demand: ReaderDemand,
    /// CDN 视频地址（恢复填充直接按 Range 续拉）
    pub cdn_url: String,
    /// 续填时沿用起播阶段生成的磁盘资源键。
    pub media_cache_key: Option<String>,
    /// 取流时的代理配置（恢复填充重建 client 用）
    pub proxy: hongguo_common::model::ProxyConfig,
}

/// 读者需求登记表：serve 在等哪个区间，填充调度器就先下哪个区间。
/// 回跳立即反映、需求满足即销、多读者按到达顺序服务（取代旧版单调只升的 `seek_hint`）。
pub struct ReaderDemand {
    /// 按到达顺序排列的登记（push 顺序即 FIFO 服务顺序）
    inner: Mutex<Vec<Ticket>>,
    next_id: AtomicU64,
}

/// 一次登记：某位读者声明需要的全部密文区间（可能多段）。
struct Ticket {
    id: u64,
    ranges: Vec<(u64, u64)>,
}

impl Default for ReaderDemand {
    fn default() -> Self {
        Self::new()
    }
}

impl ReaderDemand {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// 登记一组需要的密文区间，返回注销凭据。重复登记同一区间会得到
    /// 各自的凭据——调度器视角它们只是两个先到的同一诉求，无害。
    pub fn register(&self, ranges: &[(u64, u64)]) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner.lock().push(Ticket {
            id,
            ranges: ranges.to_vec(),
        });
        id
    }

    /// 注销一条登记。就绪、超时、条目逐出都必须调：泄漏的登记只会让
    /// 填充多下些终归要顺序下满的字节（正确性无碍），但会占着调度优先级。
    pub fn unregister(&self, id: u64) {
        self.inner.lock().retain(|t| t.id != id);
    }

    /// 是否还有未注销的读者（预取上限解绑的判定用，粗粒度即可）。
    pub fn has_waiters(&self) -> bool {
        !self.inner.lock().is_empty()
    }

    /// 最早到达、且仍有未覆盖区间的需求：返回它区间内第一个洞的起点。洞的定位必须和
    /// 覆盖表在同一把锁的视野里做，否则竞态会把调度器指到已覆盖的位置空转；全就绪（或无人等）返回 `None`。
    pub fn first_wanted(&self, sparse: &SparseBuffer) -> Option<u64> {
        let tickets = self.inner.lock();
        for t in tickets.iter() {
            for (a, b) in t.ranges.iter() {
                if let Some((gap, _)) = sparse.next_gap(*a)
                    && gap < *b
                {
                    return Some(gap);
                }
            }
        }
        None
    }
}

impl ProgressiveStream {
    /// 明文区间依赖的密文区间。
    fn needed(&self, start: u64, end: u64) -> Vec<(u64, u64)> {
        match &self.plan {
            Some(p) => p.cipher_ranges_needed(start, end),
            None => vec![(start, end)],
        }
    }

    /// 渲染明文区间 `[start, end)`。调用方先保证依赖区间就位。
    fn render(&self, start: u64, end: u64, out: &mut [u8]) {
        match &self.plan {
            Some(p) => p.render(start, end, &self.sparse, out),
            None => self.sparse.read_into(start, out),
        }
    }
}

/// 供给在线流的一次请求。
///
/// 请求早于取流完成到达时**等待**数据就绪，而不是立即报错。
pub fn serve(
    vid: &str,
    definition: u32,
    range_header: Option<&str>,
) -> Result<ProtocolResponse, String> {
    serve_inner(vid, definition, range_header, &|| false)
}

/// [`serve`] 的可取消变体：等待期间逐片检查 `cancelled`，命中即注销读者登记并立即报错返回。
/// seek 后堵在旧位置的读取必须尽快退场，否则调度器按 FIFO 先下完旧区间才轮到新位置。
pub fn serve_cancellable(
    vid: &str,
    definition: u32,
    range_header: Option<&str>,
    cancelled: impl Fn() -> bool,
) -> Result<ProtocolResponse, String> {
    serve_inner(vid, definition, range_header, &cancelled)
}

/// 读取期间持有的条目租约：条目被逐出或同键重建后，手里这份的填充必然已死亡，
/// 每个等待切片都要用 [`is_current`](Self::is_current) 复查。
pub(crate) struct EntryLease {
    cache: &'static crate::StreamCache,
    vid: String,
    definition: u32,
    pub(crate) entry: Arc<StreamEntry>,
}

impl EntryLease {
    pub(crate) fn is_current(&self) -> bool {
        self.cache
            .get(&self.vid, self.definition)
            .is_some_and(|entry| Arc::ptr_eq(&entry, &self.entry))
    }
}

/// 条目已就绪的供数模式：整集（内存明文）或渐进（稀疏密文 + 解密计划）。
pub(crate) enum EntryMode {
    Whole,
    Progressive(Arc<ProgressiveStream>),
}

/// 等条目就绪并拿到租约。`serve()`（HTTP Range）与 [`crate::read`]（字节读取）共用：
/// 两条路只是对“要哪段字节”的表达不同，就绪判定、取消/逐出语义必须完全一致。
pub(crate) fn wait_for_entry(
    vid: &str,
    definition: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<(EntryLease, EntryMode), StreamReadError> {
    let cache = crate::cache();
    let deadline = std::time::Instant::now() + Duration::from_secs(WAIT_TIMEOUT_SECS);
    // 条目未注册时分两种处置：后台还在取流（begin_fetch 已标记）就等它注册——
    // prepare 先返回 URL 再 spawn 填充，先到的 Range 请求落在这个窗口；没人在取则立即报错不白等。
    let mut entry = cache.get(vid, definition);
    while entry.is_none() {
        if cancelled() {
            return Err(StreamReadError::Cancelled);
        }
        if !cache.is_fetching(vid, definition) {
            // debug：连播/预算淘汰后 WebView 仍会对旧 URL 打并发 Range，每条一 warn 会刷爆日志；
            // 真正卡死看下方 Timeout。错误仍回给协议层（404），播放器走自愈。
            log::debug!("[Stream] 请求无对应缓存条目: {vid}@{definition}（已被逐出或从未就绪）");
            return Err(StreamReadError::Missing {
                vid: vid.to_string(),
                definition,
            });
        }
        if std::time::Instant::now() >= deadline {
            log::warn!("[Stream] {vid}@{definition} 等待取流注册条目超时（{WAIT_TIMEOUT_SECS}s）");
            return Err(StreamReadError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(50));
        entry = cache.get(vid, definition);
    }
    let lease = EntryLease {
        cache,
        vid: vid.to_string(),
        definition,
        entry: entry.expect("循环出口保证非空"),
    };

    // 两条就绪路径：整集模式等 filled、渐进模式等计划注册，取流失败回落整集时谁先就绪走谁。
    let entered = std::time::Instant::now();
    let mut polls: u64 = 0;
    loop {
        polls += 1;
        if cancelled() {
            log::debug!("[Stream] {vid}@{definition} 等待就绪前已被取消（seek 重定位）");
            return Err(StreamReadError::Cancelled);
        }
        if !lease.is_current() {
            // 连播切走会逐出当前键；与 Missing 同属预期竞态，不抬 warn
            log::debug!("[Stream] {vid}@{definition} 等待就绪时条目已被逐出/重建，立即放弃");
            return Err(StreamReadError::Replaced);
        }
        let prog = lease.entry.progressive.lock().clone();
        let filled = *lease.entry.filled.lock();
        if filled > 0 {
            if polls > 1 {
                log::debug!(
                    "[Stream][probe] mode 就绪(整集): polls={polls} 等 {:.3}s",
                    entered.elapsed().as_secs_f64()
                );
            }
            return Ok((lease, EntryMode::Whole));
        }
        if let Some(progressive) = prog {
            if polls > 1 {
                log::debug!(
                    "[Stream][probe] mode 就绪(渐进): polls={polls} 等 {:.3}s",
                    entered.elapsed().as_secs_f64()
                );
            }
            return Ok((lease, EntryMode::Progressive(progressive)));
        }
        if polls == 1 || polls.is_multiple_of(40) {
            log::debug!(
                "[Stream][probe] mode 等待中: filled={filled} prog=false 已等 {:.3}s",
                entered.elapsed().as_secs_f64()
            );
        }
        if std::time::Instant::now() >= deadline {
            log::warn!("[Stream] {vid}@{definition} 等待流就绪超时（{WAIT_TIMEOUT_SECS}s）");
            return Err(StreamReadError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn serve_inner(
    vid: &str,
    definition: u32,
    range_header: Option<&str>,
    cancelled: &dyn Fn() -> bool,
) -> Result<ProtocolResponse, String> {
    // 一集会打十几条，所以放 debug：排查「WebView 到底有没有请求这条协议」
    // 时用 RUST_LOG=debug 打开，日常不刷屏。
    log::debug!("[Stream] 请求 vid={vid} def={definition} range={range_header:?}");

    let (lease, mode) =
        wait_for_entry(vid, definition, cancelled).map_err(|error| error.to_string())?;

    match mode {
        EntryMode::Whole => {
            // 锁序与 store 一致（buffer → size），避免与写入侧交叉等待。
            let buffer = lease.entry.buffer.lock();
            let size = *lease.entry.size.lock();
            Ok(respond(&buffer, parse_range(range_header, size), size))
        }
        EntryMode::Progressive(progressive) => {
            serve_progressive(&progressive, range_header, &lease, cancelled)
        }
    }
}

/// 渐进模式应答：把明文 Range 映射回密文区间，等齐、解密、拼装。
/// 等待期间条目被逐出/同键重建（`lease` 失效）或 `cancelled` 命中（seek 重定位）都立即放弃并注销读者登记。
fn serve_progressive(
    prog: &Arc<ProgressiveStream>,
    range_header: Option<&str>,
    lease: &EntryLease,
    cancelled: &dyn Fn() -> bool,
) -> Result<ProtocolResponse, String> {
    let plain_len = prog.plain_len;
    let (start, end) = match parse_range(range_header, plain_len) {
        RangeSpec::Full => (0, plain_len),
        RangeSpec::Closed { start, end } => (start, end + 1),
        RangeSpec::Open { start } => {
            // 窗口 0 与整集模式同义（一次给到末尾）；不能原样参与加法——
            // end==start 会让下方 end-1 下溢（start=0 时 debug panic）
            (
                start,
                open_range_end(start, progressive_window(), plain_len),
            )
        }
        RangeSpec::Unsatisfiable => {
            return Ok((
                416,
                vec![
                    ("Content-Range".into(), format!("bytes */{plain_len}")),
                    ("Content-Type".into(), "text/plain".into()),
                ],
                Vec::new(),
            ));
        }
    };

    let body =
        read_plain_range(prog, start, end, lease, cancelled).map_err(|error| error.to_string())?;

    if range_header.is_some() {
        Ok((206, partial_headers(start, end - 1, plain_len), body))
    } else {
        Ok((
            200,
            vec![
                ("Content-Type".into(), "video/mp4".into()),
                ("Accept-Ranges".into(), "bytes".into()),
                ("Content-Length".into(), body.len().to_string()),
                // 与 206 同理：流内容随时在变，禁掉 WebView2 的响应缓存
                ("Cache-Control".into(), "no-store".into()),
            ],
            body,
        ))
    }
}

/// 读一段明文 `[start, end)`：等依赖的密文区间、登记读者需求、惰性解密。
/// `serve()` 与 [`crate::read`] 的字节读取共用这一份实现，HTTP 头/206 是壳层语义。
pub(crate) fn read_plain_range(
    prog: &ProgressiveStream,
    start: u64,
    end: u64,
    lease: &EntryLease,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, StreamReadError> {
    if start >= end {
        return Ok(Vec::new());
    }

    let needed = prog.needed(start, end);
    if !prog.sparse.wait_cover(&needed, Duration::ZERO) {
        let deadline = std::time::Instant::now() + Duration::from_secs(WAIT_TIMEOUT_SECS);
        // 切片等待：数据到达由条件变量即时唤醒；切片只是给「条目被逐出」与总超时留检查点。
        const WAIT_SLICE: Duration = Duration::from_millis(200);
        let ticket = prog.demand.register(&needed);
        let outcome = loop {
            if prog.sparse.wait_cover(&needed, WAIT_SLICE) {
                break Ok(());
            }
            if cancelled() {
                log::debug!(
                    "[Stream] 等待明文区间 {start}-{end} 期间被取消（seek 重定位），注销读者登记退场"
                );
                break Err(StreamReadError::Cancelled);
            }
            if !lease.is_current() {
                log::debug!(
                    "[Stream] 等待明文区间 {start}-{end} 时条目已被逐出/重建，立即放弃（不再等满 {WAIT_TIMEOUT_SECS}s）"
                );
                break Err(StreamReadError::Replaced);
            }
            if std::time::Instant::now() >= deadline {
                log::warn!(
                    "[Stream] 等待明文区间 {start}-{end} 的密文就绪超时（{WAIT_TIMEOUT_SECS}s），填充可能停摆"
                );
                break Err(StreamReadError::Timeout);
            }
        };
        prog.demand.unregister(ticket);
        outcome?;
    }

    let mut body = vec![0u8; (end - start) as usize];
    prog.render(start, end, &mut body);
    Ok(body)
}

/// 渐进模式的开放 Range 窗口大小，可用 `HONGGUO_STREAM_WINDOW` 覆盖。
/// `0` = 一次给到末尾（与整集模式 [`stream_window`] 同义）。
fn progressive_window() -> u64 {
    static WINDOW: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *WINDOW.get_or_init(|| {
        std::env::var("HONGGUO_STREAM_WINDOW")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(PROGRESSIVE_WINDOW)
    })
}

/// 开放式 Range 的本次供给终点：窗口 0 给到末尾，否则按窗口截断。
fn open_range_end(start: u64, window: u64, plain_len: u64) -> u64 {
    if window == 0 {
        plain_len
    } else {
        (start + window).min(plain_len)
    }
}

/// 开放式 Range 单次供给的窗口大小。`0` = 一次给到末尾（默认），
/// 可用 `HONGGUO_STREAM_WINDOW` 覆盖。
fn stream_window() -> u64 {
    static WINDOW: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *WINDOW.get_or_init(|| {
        std::env::var("HONGGUO_STREAM_WINDOW")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    })
}

/// 按 Range 切一段数据出来（整集模式）。只在 `size` 已经就绪之后调用——那意味着
/// 整集明文全部在内存里，每一种 Range 都一定切得出来，没有「等数据」。
fn respond(buffer: &[u8], range: RangeSpec, size: u64) -> ProtocolResponse {
    match range {
        RangeSpec::Full => (
            200,
            vec![
                ("Content-Type".into(), "video/mp4".into()),
                ("Accept-Ranges".into(), "bytes".into()),
                ("Content-Length".into(), size.to_string()),
                // 与 206 同理：禁掉 WebView2 的响应缓存
                ("Cache-Control".into(), "no-store".into()),
            ],
            buffer.to_vec(),
        ),
        RangeSpec::Closed { start, end } => (
            206,
            partial_headers(start, end, size),
            buffer[start as usize..=end as usize].to_vec(),
        ),
        // 开放式：默认一次给到文件末尾；设 `HONGGUO_STREAM_WINDOW`（字节）可切成只回一窗。
        RangeSpec::Open { start } => {
            let end = match stream_window() {
                0 => size - 1,
                w => (start + w).min(size) - 1,
            };
            (
                206,
                partial_headers(start, end, size),
                buffer[start as usize..=end as usize].to_vec(),
            )
        }
        RangeSpec::Unsatisfiable => (
            416,
            vec![
                ("Content-Range".into(), format!("bytes */{size}")),
                ("Content-Type".into(), "text/plain".into()),
            ],
            Vec::new(),
        ),
    }
}

#[cfg(test)]
#[path = "progressive_tests.rs"]
mod tests;
