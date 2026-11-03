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
            log::warn!("[Stream] 请求无对应缓存条目: {vid}@{definition}（已被逐出或从未就绪）");
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
            log::info!("[Stream] {vid}@{definition} 等待就绪前已被取消（seek 重定位）");
            return Err(StreamReadError::Cancelled);
        }
        if !lease.is_current() {
            log::warn!("[Stream] {vid}@{definition} 等待就绪时条目已被逐出/重建，立即放弃");
            return Err(StreamReadError::Replaced);
        }
        let prog = lease.entry.progressive.lock().clone();
        let filled = *lease.entry.filled.lock();
        if filled > 0 {
            if polls > 1 {
                log::info!(
                    "[Stream][probe] mode 就绪(整集): polls={polls} 等 {:.3}s",
                    entered.elapsed().as_secs_f64()
                );
            }
            return Ok((lease, EntryMode::Whole));
        }
        if let Some(progressive) = prog {
            if polls > 1 {
                log::info!(
                    "[Stream][probe] mode 就绪(渐进): polls={polls} 等 {:.3}s",
                    entered.elapsed().as_secs_f64()
                );
            }
            return Ok((lease, EntryMode::Progressive(progressive)));
        }
        if polls == 1 || polls.is_multiple_of(40) {
            log::info!(
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
                log::info!(
                    "[Stream] 等待明文区间 {start}-{end} 期间被取消（seek 重定位），注销读者登记退场"
                );
                break Err(StreamReadError::Cancelled);
            }
            if !lease.is_current() {
                log::warn!(
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
mod tests {
    use super::*;
    use crate::StreamCache;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn zero_window_means_till_end_not_underflow() {
        assert_eq!(open_range_end(0, 0, 1_000), 1_000);
        assert_eq!(open_range_end(500, 0, 1_000), 1_000);
        assert_eq!(open_range_end(0, 100, 1_000), 100);
        assert_eq!(open_range_end(900, 100, 1_000), 1_000);
        assert_eq!(open_range_end(999, 100, 1_000), 1_000);
    }

    fn ready_stream(vid: &str, definition: u32, len: usize) {
        let c = crate::cache();
        c.remove(vid, definition);
        c.store(
            vid,
            definition,
            &(0..len).map(|i| (i % 251) as u8).collect::<Vec<u8>>(),
        );
    }

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn serves_full_body_after_fill() {
        ready_stream("v-full", 1080, 4096);
        let (status, headers, body) = serve("v-full", 1080, None).expect("整文件请求应成功");
        assert_eq!(status, 200);
        assert_eq!(body.len(), 4096);
        assert_eq!(header(&headers, "Content-Type"), Some("video/mp4"));
        assert_eq!(header(&headers, "Accept-Ranges"), Some("bytes"));
        assert_eq!(header(&headers, "Content-Length"), Some("4096"));
        assert_eq!(header(&headers, "Cache-Control"), Some("no-store"));
    }

    #[test]
    fn serves_closed_range_for_seeking() {
        ready_stream("v-closed", 1080, 4096);
        let (status, headers, body) = serve("v-closed", 1080, Some("bytes=100-199")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body.len(), 100);
        assert_eq!(
            header(&headers, "Content-Range"),
            Some("bytes 100-199/4096")
        );
        assert_eq!(header(&headers, "Content-Length"), Some("100"));
    }

    #[test]
    fn open_range_returns_everything_from_the_offset() {
        ready_stream("v-open", 1080, 4096);
        let (status, headers, body) = serve("v-open", 1080, Some("bytes=0-")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body.len(), 4096);
        assert_eq!(header(&headers, "Content-Range"), Some("bytes 0-4095/4096"));
        assert_eq!(header(&headers, "Content-Length"), Some("4096"));
    }

    #[test]
    fn open_range_after_a_seek_also_reaches_the_end() {
        ready_stream("v-seek", 1080, 4096);
        let (status, _, _) = serve("v-seek", 1080, Some("bytes=1146880-")).unwrap();
        assert_eq!(status, 416, "超出总长的偏移仍应是不可满足");

        ready_stream("v-seek", 1080, 4096);
        let (status, headers, body) = serve("v-seek", 1080, Some("bytes=3000-")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body.len(), 1096);
        assert_eq!(
            header(&headers, "Content-Range"),
            Some("bytes 3000-4095/4096")
        );
    }

    #[test]
    fn unknown_stream_is_an_error_not_a_hang() {
        let err = serve("never-prepared", 1080, Some("bytes=0-")).expect_err("未准备的流应报错");
        assert!(err.contains("没有正在准备的在线流"), "实际: {err}");
    }

    #[test]
    fn out_of_bounds_range_is_416() {
        ready_stream("v-416", 1080, 100);
        let (status, headers, body) = serve("v-416", 1080, Some("bytes=500-600")).unwrap();
        assert_eq!(status, 416);
        assert_eq!(header(&headers, "Content-Range"), Some("bytes */100"));
        assert!(body.is_empty());
    }

    #[test]
    fn serve_fails_fast_when_entry_is_replaced_midwait() {
        let c = crate::cache();
        c.remove("v-stale", 1080);
        c.set_definitions("v-stale", 1080, &[]);
        let handle = std::thread::spawn(|| serve("v-stale", 1080, Some("bytes=0-")));
        std::thread::sleep(Duration::from_millis(300));
        c.remove("v-stale", 1080);
        c.set_definitions("v-stale", 1080, &[]);
        let started = std::time::Instant::now();
        let result = handle.join().expect("serve 线程不应 panic");
        assert!(result.is_err(), "被替换的流应报错而不是返回空数据");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "应快速失败，不该等满 30s 超时"
        );
    }

    #[test]
    fn switching_quality_leaves_the_previous_one_serving_its_own_bytes() {
        const FILL_1080: u8 = 0xA1;
        const FILL_720: u8 = 0xB2;
        let c = crate::cache();
        c.remove("v-switch", 1080);
        c.remove("v-switch", 720);
        c.store("v-switch", 1080, &[FILL_1080; 512]);
        c.store("v-switch", 720, &[FILL_720; 256]);

        let (status, _, body) = serve("v-switch", 1080, Some("bytes=0-99")).unwrap();
        assert_eq!(status, 206);
        assert!(
            body.iter().all(|b| *b == FILL_1080),
            "1080 应继续供自己那份字节"
        );

        let (status, _, body) = serve("v-switch", 720, Some("bytes=0-99")).unwrap();
        assert_eq!(status, 206);
        assert!(body.iter().all(|b| *b == FILL_720), "720 应供自己那份字节");
    }

    #[test]
    fn a_definition_that_was_never_fetched_is_not_served() {
        ready_stream("v-only-1080", 1080, 256);
        let err = serve("v-only-1080", 720, Some("bytes=0-")).expect_err("没下过的档位应报错");
        assert!(err.contains("没有正在准备的在线流"), "实际: {err}");
    }

    fn progressive_fixture(vid: &str, def: u32) -> (Arc<SparseBuffer>, Vec<u8>, Vec<u8>, u64) {
        use hongguo_video::decrypt_buffer::decrypt_mp4_buffer;
        use hongguo_video::streaming::StreamingPlan;
        use hongguo_video::streaming_tests::assemble;

        let (_plain, cipher, key) = assemble(true);
        let reference = decrypt_mp4_buffer(&cipher, &key).expect("参照明文应可得");
        let len = cipher.len() as u64;
        let (abs, size) =
            hongguo_video::streaming::locate_moov(&cipher, 0, len).expect("应定位 moov");
        let region = &cipher[abs as usize..(abs + size) as usize];
        let plan = StreamingPlan::build(region, abs, len, &key).expect("计划应构建成功");
        let plain_len = plan.plain_len();
        let sparse = SparseBuffer::new(len);
        let prog = ProgressiveStream {
            sparse: sparse.clone(),
            plan: Some(Arc::new(plan)),
            plain_len,
            demand: ReaderDemand::new(),
            cdn_url: "https://cdn/example.mp4".into(),
            proxy: Default::default(),
        };
        let c = crate::cache();
        c.remove(vid, def);
        assert!(c.begin_fetch(vid, def));
        c.end_fetch(vid, def);
        assert!(c.set_progressive(vid, def, Arc::new(prog)));
        (sparse, cipher, reference, plain_len)
    }

    #[test]
    fn progressive_serves_decrypted_range_as_data_arrives() {
        let (sparse, cipher, reference, plain_len) = progressive_fixture("v-prog", 720);

        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            sparse.write(0, &cipher);
        });
        let (status, headers, body) = serve("v-prog", 720, Some("bytes=0-99")).expect("应成功");
        writer.join().unwrap();

        assert_eq!(status, 206);
        assert_eq!(body, &reference[0..100], "应答必须是解密后的明文");
        assert_eq!(
            header(&headers, "Content-Range"),
            Some(format!("bytes 0-99/{plain_len}").as_str())
        );
    }

    #[test]
    fn progressive_open_range_returns_a_window_from_the_offset() {
        let (sparse, cipher, reference, plain_len) = progressive_fixture("v-prog-open", 720);
        sparse.write(0, &cipher);

        let (status, headers, body) = serve("v-prog-open", 720, Some("bytes=30-")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body, &reference[30..]);
        assert_eq!(
            header(&headers, "Content-Range"),
            Some(format!("bytes 30-{}/{plain_len}", plain_len - 1).as_str())
        );
    }

    #[test]
    fn progressive_beyond_plain_len_is_416() {
        let (sparse, cipher, _reference, plain_len) = progressive_fixture("v-prog-416", 720);
        sparse.write(0, &cipher);

        let (status, headers, body) = serve("v-prog-416", 720, Some("bytes=999999999-")).unwrap();
        assert_eq!(status, 416);
        assert_eq!(
            header(&headers, "Content-Range"),
            Some(format!("bytes */{plain_len}").as_str())
        );
        assert!(body.is_empty());
    }

    #[test]
    fn progressive_identity_stream_serves_raw_bytes() {
        let c = crate::cache();
        let data: Vec<u8> = (0..600u32).map(|i| (i % 251) as u8).collect();
        let sparse = SparseBuffer::new(data.len() as u64);
        sparse.write(0, &data);
        let prog = ProgressiveStream {
            sparse,
            plan: None,
            plain_len: data.len() as u64,
            demand: ReaderDemand::new(),
            cdn_url: "https://cdn/example.mp4".into(),
            proxy: Default::default(),
        };
        c.remove("v-prog-raw", 720);
        assert!(c.begin_fetch("v-prog-raw", 720));
        c.end_fetch("v-prog-raw", 720);
        assert!(c.set_progressive("v-prog-raw", 720, Arc::new(prog)));

        let (status, _, body) = serve("v-prog-raw", 720, Some("bytes=100-199")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body, &data[100..200]);
    }

    #[test]
    fn progressive_registers_demand_while_waiting() {
        let (sparse, cipher, _reference, _len) = progressive_fixture("v-prog-demand", 720);

        let server = std::thread::spawn(move || {
            let r = serve("v-prog-demand", 720, Some("bytes=10-19"));
            (r.is_ok(), r.map(|(_, _, b)| b.len()).unwrap_or(0))
        });
        std::thread::sleep(Duration::from_millis(150));

        let c = crate::cache();
        let prog = c
            .get("v-prog-demand", 720)
            .and_then(|e| e.progressive.lock().clone())
            .expect("渐进条目应在");
        assert!(prog.demand.has_waiters(), "等待期间应留下需求登记");
        assert_eq!(
            prog.demand.first_wanted(&prog.sparse),
            Some(10),
            "第一个需求洞应在缺口起点"
        );

        sparse.write(0, &cipher);
        let (ok, len) = server.join().unwrap();
        assert!(ok);
        assert_eq!(len, 10);
        assert!(!prog.demand.has_waiters(), "应答完成后登记应注销");
    }

    #[test]
    fn cancellable_serve_exits_promptly_and_unregisters_demand() {
        let (sparse, cipher, _reference, _len) = progressive_fixture("v-prog-cancel", 720);

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_cb = Arc::clone(&cancel);
        let server = std::thread::spawn(move || {
            serve_cancellable("v-prog-cancel", 720, Some("bytes=10-19"), || {
                cancel_cb.load(Ordering::Relaxed)
            })
        });
        std::thread::sleep(Duration::from_millis(150));
        let c = crate::cache();
        let prog = c
            .get("v-prog-cancel", 720)
            .and_then(|e| e.progressive.lock().clone())
            .expect("渐进条目应在");
        assert!(prog.demand.has_waiters(), "等待期间应留下需求登记");

        let started = std::time::Instant::now();
        cancel.store(true, Ordering::Relaxed);
        let result = server.join().expect("serve 线程不应 panic");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "取消应快速生效，实际 {:?}",
            started.elapsed()
        );
        assert!(result.is_err(), "取消的读取应报错而不是返回数据");

        assert!(!prog.demand.has_waiters(), "取消后读者登记应注销");

        sparse.write(0, &cipher);
        let (status, _, body) = serve("v-prog-cancel", 720, Some("bytes=10-19")).expect("应成功");
        assert_eq!(status, 206);
        assert_eq!(body, &cipher[10..20]);
    }

    #[test]
    fn progressive_is_reported_ready_before_any_data() {
        let c = crate::cache();
        c.remove("v-prog-ready", 720);
        let sparse = SparseBuffer::new(16);
        let prog = ProgressiveStream {
            sparse,
            plan: None,
            plain_len: 16,
            demand: ReaderDemand::new(),
            cdn_url: "https://cdn/example.mp4".into(),
            proxy: Default::default(),
        };
        assert!(c.begin_fetch("v-prog-ready", 720));
        c.end_fetch("v-prog-ready", 720);
        assert!(c.set_progressive("v-prog-ready", 720, Arc::new(prog)));
        assert!(c.ready("v-prog-ready", 720).is_some(), "注册即就绪");
    }

    #[test]
    fn progressive_registration_does_not_resurrect_an_evicted_entry() {
        let c = crate::cache();
        c.remove("v-prog-gone", 720);
        let prog = ProgressiveStream {
            sparse: SparseBuffer::new(16),
            plan: None,
            plain_len: 16,
            demand: ReaderDemand::new(),
            cdn_url: "https://cdn/example.mp4".into(),
            proxy: Default::default(),
        };
        assert!(!c.set_progressive("v-prog-gone", 720, Arc::new(prog)));
        assert!(c.get("v-prog-gone", 720).is_none(), "不得复活条目");
    }

    #[test]
    fn set_progressive_yields_to_whole_mode_data() {
        let c = StreamCache::default();
        c.store("v-race", 720, &[1u8; 8]);
        let prog = ProgressiveStream {
            sparse: SparseBuffer::new(4),
            plan: None,
            plain_len: 4,
            demand: ReaderDemand::new(),
            cdn_url: "https://cdn/example.mp4".into(),
            proxy: Default::default(),
        };
        assert!(!c.set_progressive("v-race", 720, Arc::new(prog)));
    }

    use crate::read::StreamReadError;
    use crate::{read_range, read_range_cancellable, total_len};

    #[test]
    fn byte_reads_whole_mode_range_like_serve() {
        ready_stream("v-bytes-whole", 1080, 4096);
        assert_eq!(total_len("v-bytes-whole", 1080), Some(4096));

        let chunk = read_range("v-bytes-whole", 1080, 100, Some(50)).expect("闭区间应成功");
        let reference: Vec<u8> = (100..150).map(|i| (i % 251) as u8).collect();
        assert_eq!(chunk.total, 4096);
        assert_eq!(chunk.start, 100);
        assert_eq!(chunk.bytes, reference);

        let (status, _, body) = serve("v-bytes-whole", 1080, Some("bytes=100-149")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body, chunk.bytes);
    }

    #[test]
    fn byte_read_range_clamps_at_end_and_allows_eof_read() {
        ready_stream("v-bytes-clamp", 1080, 4096);

        let tail = read_range("v-bytes-clamp", 1080, 4090, Some(100)).expect("越过末尾应截断");
        assert_eq!(tail.bytes.len(), 6, "只应回末尾剩余字节");
        assert_eq!(tail.total, 4096);

        let eof = read_range("v-bytes-clamp", 1080, 4096, None).expect("EOF 读应成功");
        assert!(eof.bytes.is_empty());
        assert_eq!(eof.start, 4096);

        let empty = read_range("v-bytes-clamp", 1080, 4090, Some(0)).expect("零长读应成功");
        assert!(empty.bytes.is_empty());

        let error = read_range("v-bytes-clamp", 1080, 4097, Some(1)).expect_err("越界应报错");
        assert!(matches!(
            error,
            StreamReadError::OutOfRange {
                start: 4097,
                total: 4096
            }
        ));
    }

    #[test]
    fn byte_progressive_len_is_known_before_any_data() {
        let (sparse, cipher, reference, plain_len) = progressive_fixture("v-bytes-prog", 720);
        assert_eq!(total_len("v-bytes-prog", 720), Some(plain_len));

        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            sparse.write(0, &cipher);
        });
        let chunk = read_range("v-bytes-prog", 720, 10, Some(20)).expect("渐进读应成功");
        writer.join().unwrap();
        assert_eq!(chunk.bytes, &reference[10..30]);
        assert_eq!(chunk.total, plain_len);
    }

    #[test]
    fn byte_progressive_read_cancel_unregisters_demand() {
        let (sparse, cipher, _reference, _len) = progressive_fixture("v-bytes-cancel", 720);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_cb = Arc::clone(&cancel);
        let reader = std::thread::spawn(move || {
            read_range_cancellable("v-bytes-cancel", 720, 10, Some(10), || {
                cancel_cb.load(Ordering::Relaxed)
            })
        });
        std::thread::sleep(Duration::from_millis(150));

        let c = crate::cache();
        let prog = c
            .get("v-bytes-cancel", 720)
            .and_then(|entry| entry.progressive.lock().clone())
            .expect("渐进条目应在");
        assert!(prog.demand.has_waiters(), "等待期间应留下读者登记");

        cancel.store(true, Ordering::Relaxed);
        let started = std::time::Instant::now();
        let error = reader
            .join()
            .expect("读取线程不应 panic")
            .expect_err("应被取消");
        assert!(matches!(error, StreamReadError::Cancelled));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "取消应立即生效，实际 {:?}",
            started.elapsed()
        );
        assert!(!prog.demand.has_waiters(), "取消后读者登记应注销");

        sparse.write(0, &cipher);
        let chunk = read_range("v-bytes-cancel", 720, 10, Some(10)).expect("取消后应可重读");
        assert_eq!(chunk.bytes, &cipher[10..20]);
    }

    #[test]
    fn byte_missing_entry_is_an_error_not_a_hang() {
        let error = read_range("v-bytes-never", 720, 0, Some(1)).expect_err("未准备的流应报错");
        assert!(matches!(error, StreamReadError::Missing { .. }));
        assert_eq!(total_len("v-bytes-never", 720), None);
    }

    #[test]
    fn byte_reads_identity_progressive_stream() {
        let c = crate::cache();
        let data: Vec<u8> = (0..600u32).map(|i| (i % 251) as u8).collect();
        let sparse = SparseBuffer::new(data.len() as u64);
        sparse.write(0, &data);
        let prog = ProgressiveStream {
            sparse,
            plan: None,
            plain_len: data.len() as u64,
            demand: ReaderDemand::new(),
            cdn_url: "https://cdn/example.mp4".into(),
            proxy: Default::default(),
        };
        c.remove("v-bytes-raw", 720);
        assert!(c.begin_fetch("v-bytes-raw", 720));
        c.end_fetch("v-bytes-raw", 720);
        assert!(c.set_progressive("v-bytes-raw", 720, Arc::new(prog)));

        assert_eq!(total_len("v-bytes-raw", 720), Some(600));
        let chunk = read_range("v-bytes-raw", 720, 100, Some(100)).unwrap();
        assert_eq!(chunk.bytes, &data[100..200]);
    }

    #[test]
    fn reader_demand_serves_fifo_and_clears_on_unregister() {
        let d = ReaderDemand::new();
        let sparse = SparseBuffer::new(100);
        sparse.write(0, &[0u8; 10]);

        let t1 = d.register(&[(0, 10)]);
        let t2 = d.register(&[(80, 90)]);
        assert_eq!(
            d.first_wanted(&sparse),
            Some(80),
            "就绪的登记跳过，轮到后到者"
        );

        d.unregister(t2);
        assert_eq!(d.first_wanted(&sparse), None, "注销后不再指向它");
        assert!(d.has_waiters(), "t1 仍在");
        d.unregister(t1);
        assert!(!d.has_waiters(), "全部注销后无等待者");
    }

    #[test]
    fn reader_demand_lands_inside_mid_range_hole() {
        let d = ReaderDemand::new();
        let sparse = SparseBuffer::new(100);
        sparse.write(10, &[0u8; 10]);
        sparse.write(25, &[0u8; 5]);
        d.register(&[(10, 40)]);
        assert_eq!(d.first_wanted(&sparse), Some(20));
        sparse.write(20, &[0u8; 5]);
        assert_eq!(
            d.first_wanted(&sparse),
            Some(30),
            "补齐后应指到区间内下一个洞"
        );
    }

    #[test]
    fn reader_demand_fifo_never_starves_early_reader() {
        let d = ReaderDemand::new();
        let sparse = SparseBuffer::new(100);
        d.register(&[(60, 70)]);
        d.register(&[(5, 10)]);
        assert_eq!(
            d.first_wanted(&sparse),
            Some(60),
            "先到者优先，即使它的区间更靠后"
        );
    }
}
