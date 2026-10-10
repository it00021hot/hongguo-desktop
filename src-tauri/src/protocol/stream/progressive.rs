//! 渐进流数据面：`ProgressiveStream` 数据结构 + 协议层 Range 应答 + 窗口策略。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use crate::domain::mp4::streaming::{SparseBuffer, StreamingPlan};
use crate::protocol::range::{ProtocolResponse, RangeSpec, parse_range, partial_headers};

/// 等待数据就绪的最长时间（秒）。超时返回 502，避免请求永久挂起。
const WAIT_TIMEOUT_SECS: u64 = 30;

/// 渐进式在线流的开放 Range 默认窗口：一次回 4MB。
///
/// 整集模式的窗口语义见 [`respond`]（默认一次给到底）；渐进模式必须开窗口，
/// 否则 `bytes=0-` 要等整集下载完才应答，渐进就没有意义了。
const PROGRESSIVE_WINDOW: u64 = 4 * 1024 * 1024;

/// 渐进式在线流：稀疏密文 + 解密计划，协议层按 Range 惰性解密。
///
/// `plan` 为 `None` 表示未加密流（官网兜底链路），明文就是密文本身。
///
/// `cdn_url`/`proxy` 供「预取只填头部、转为当前集后续填余下」的恢复填充用
/// （见 [`super::super::service` 侧 fill]）——没有它们，续填就得把取流表
/// 那次接口调用整个重放一遍。
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
    pub proxy: crate::domain::model::ProxyConfig,
}

/// 读者需求登记表：serve 在等哪个区间，填充调度器就先下哪个区间。
///
/// 取代旧版的 `seek_hint: AtomicU64` 单值提示。单值提示有三处先天缺陷，
/// 正是「回绕补洞死循环」的根因：
/// - `fetch_max` 单调只升：播放器往回 seek 之后提示**降不下来**，
///   填充永远追着一个不再需要的高位区间跑；
/// - 从不清零：需求满足后提示还留在原地，持续把回绕后的游标顶回去，
///   洞在提示之前时填充就在「回绕 → 顶回 → 回绕」里空转；
/// - 标量只有一个位置：并发的读者（解封装探针 + 播放读）互相顶掉彼此。
///
/// 登记表把「谁在等哪段」原样交给调度器：回跳立即反映、满足即销、
/// 多读者按到达顺序服务。填充侧的调度策略见
/// `play_service::online::fill_remaining_with_client`。
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

    /// 最早到达、且仍有未覆盖区间的需求：返回它**区间内第一个洞**的起点。
    ///
    /// 传 `sparse` 而不是让调用方自己查覆盖：洞的定位必须和覆盖表在同一把
    /// 锁的视野里做，否则「查完已就绪、实际又有洞」的竞态会把调度器指到
    /// 已覆盖的位置空转一轮。全就绪（或无人等）返回 `None`，调度器退回
    /// 顺序填充。
    pub fn first_wanted(&self, sparse: &SparseBuffer) -> Option<u64> {
        let tickets = self.inner.lock();
        for t in tickets.iter() {
            for (a, b) in t.ranges.iter() {
                // 区间内的第一个洞：next_gap(a) 给出 a 起第一个未覆盖处，
                // 落在 (a,b) 之外说明这段已就绪，试下一段
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
    // 一集会打十几条，所以放 debug：排查「WebView 到底有没有请求这条协议」
    // 时用 RUST_LOG=debug 打开，日常不刷屏。
    log::debug!("[Stream] 请求 vid={vid} def={definition} range={range_header:?}");

    let c = crate::service::play_service::online::cache();
    // 压根没有这一档：说明它已经被清掉了（或从没起播过）。
    // 这种情况必须立刻报错，不能进等待——等满 30 秒才告诉用户「没这流」，
    // 在切档的语境下就是一次白等。
    let entry = match c.get(vid, definition) {
        Some(e) => e,
        None => {
            // 播放中的 video 还会持续发 Range 请求，这里一 404 它就直接死。
            // 必须留痕：否则用户只看到「视频处理失败」，后端却一片安静。
            log::warn!("[Stream] 请求无对应缓存条目: {vid}@{definition}（已被逐出或从未就绪）");
            return Err(format!("没有正在准备的在线流: {vid}@{definition}"));
        }
    };

    // 「这条流还是不是眼前这条」：条目被逐出或同键重建（新 Arc）后，手里这份
    // 的填充必然已经死亡，继续等只会等满 30s——立即报错，让前端的自动重试
    // 尽快接手（重新 prepare 命中新条目，续播位置不丢）。
    let still_current = {
        let vid = vid.to_string();
        let entry = entry.clone();
        move || {
            c.get(&vid, definition)
                .is_some_and(|e| Arc::ptr_eq(&e, &entry))
        }
    };

    // 两条路：整集模式（数据一次性原子写进 buffer）等 filled；
    // 渐进模式等计划注册（plain_len 已知即可应答，字节按区间再等）。
    // 取流失败回落整集时，两条都可能出现，谁先就绪走谁。
    let deadline = std::time::Instant::now() + Duration::from_secs(WAIT_TIMEOUT_SECS);
    let mode = loop {
        if !still_current() {
            log::warn!("[Stream] {vid}@{definition} 等待就绪时条目已被逐出/重建，立即放弃");
            return Err("在线流已被替换，请重试".to_string());
        }
        let prog = entry.progressive.lock().clone();
        if *entry.filled.lock() > 0 {
            break None;
        }
        if let Some(p) = prog {
            break Some(p);
        }
        if std::time::Instant::now() >= deadline {
            log::warn!("[Stream] {vid}@{definition} 等待流就绪超时（{WAIT_TIMEOUT_SECS}s）");
            return Err("等待流就绪超时".to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    match mode {
        // 整集模式：size 只在 store 里从 0 变成真实值一次，之后再不变
        None => {
            let size = *entry.size.lock();
            let buffer = entry.buffer.lock();
            Ok(respond(&buffer, parse_range(range_header, size), size))
        }
        Some(prog) => serve_progressive(&prog, range_header, &still_current),
    }
}

/// 渐进模式应答：把明文 Range 映射回密文区间，等齐、解密、拼装。
///
/// `still_current` 为假（条目被逐出/同键重建）时立即放弃等待——手里的
/// 渐进流已经没人喂了，等满超时只是把断流拖长成 30 秒黑屏。
fn serve_progressive(
    prog: &Arc<ProgressiveStream>,
    range_header: Option<&str>,
    still_current: &impl Fn() -> bool,
) -> Result<ProtocolResponse, String> {
    let plain_len = prog.plain_len;
    let (start, end) = match parse_range(range_header, plain_len) {
        RangeSpec::Full => (0, plain_len),
        RangeSpec::Closed { start, end } => (start, end + 1),
        RangeSpec::Open { start } => {
            // 窗口 0 与整集模式的 stream_window 同义：一次给到末尾。
            // 不能原样参与加法——end==start 会让下方 end-1 下溢（start=0
            // 时 debug panic，release 出非法 Content-Range）
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

    // 等依赖的密文区间。先把需求登记进 ReaderDemand（填充调度器据此
    // 优先下这一段），再在覆盖表的条件变量上等：字节一落盘立即唤醒。
    let needed = prog.needed(start, end);
    if !prog.sparse.wait_cover(&needed, Duration::ZERO) {
        let deadline = std::time::Instant::now() + Duration::from_secs(WAIT_TIMEOUT_SECS);
        // 切片等待：数据到达由条件变量即时唤醒；切片只是给「条目被逐出」
        // 与总超时留个检查点，不再是 50ms 轮询。
        const WAIT_SLICE: Duration = Duration::from_millis(200);
        let ticket = prog.demand.register(&needed);
        let outcome = loop {
            if prog.sparse.wait_cover(&needed, WAIT_SLICE) {
                break Ok(());
            }
            if !still_current() {
                log::warn!(
                    "[Stream] 等待明文区间 {start}-{end} 时条目已被逐出/重建，立即放弃（不再等满 {WAIT_TIMEOUT_SECS}s）"
                );
                break Err("在线流已被替换，请重试".to_string());
            }
            if std::time::Instant::now() >= deadline {
                // 典型成因：填充线程停摆或网络断供，播放追上了填充前沿
                log::warn!(
                    "[Stream] 等待明文区间 {start}-{end} 的密文就绪超时（{WAIT_TIMEOUT_SECS}s），填充可能停摆"
                );
                break Err("等待流就绪超时".to_string());
            }
        };
        prog.demand.unregister(ticket);
        outcome?;
    }

    let mut body = vec![0u8; (end - start) as usize];
    prog.render(start, end, &mut body);

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

/// 开放式 Range 单次供给的窗口大小。`0` = 一次给到末尾（默认）。
///
/// 见 [`respond`] 里 `RangeSpec::Open` 分支的说明：窗口大小可调，默认一次给完。
fn stream_window() -> u64 {
    static WINDOW: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *WINDOW.get_or_init(|| {
        std::env::var("HONGGUO_STREAM_WINDOW")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    })
}

/// 按 Range 切一段数据出来（整集模式）。
///
/// 只在 `size` 已经就绪之后调用，而那意味着整集明文全部在内存里了
/// （见 [`super::StreamCache::store`]），所以每一种 Range 都一定切得出来，没有「等数据」。
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
        // 开放式：默认一次给到文件末尾。设 `HONGGUO_STREAM_WINDOW`（字节）会
        // 切成「只回一窗」——这是上游 Electron 版的做法（`SERVE_WINDOW = 512KB`）。
        //
        // 本项目历史上试过分段回、因为 seek 时请求乱序而回滚过；用真实 WebView2
        // 重测后（`bytes=0-` → `524288-` → `1048576-` … 整条序列）**没有复现**，
        // seek 到 60% 照常推进、无 MediaError。所以这里保留开关，让窗口大小
        // 可调；数据侧仍然是整集下完才交给协议。
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
    use crate::protocol::stream::StreamCache;

    /// 回归：`HONGGUO_STREAM_WINDOW=0` 曾让开放 Range 的 end==start，
    /// `end-1` 在 start=0 时 u64 下溢（debug panic / release 非法头）。
    /// 现约定窗口 0 与整集模式同义：一次给到末尾。
    #[test]
    fn zero_window_means_till_end_not_underflow() {
        assert_eq!(open_range_end(0, 0, 1_000), 1_000);
        assert_eq!(open_range_end(500, 0, 1_000), 1_000);
        assert_eq!(open_range_end(0, 100, 1_000), 100);
        assert_eq!(open_range_end(900, 100, 1_000), 1_000);
        assert_eq!(open_range_end(999, 100, 1_000), 1_000);
    }

    /// 造一个「已填好 size + 全部字节」的条目。
    fn ready_stream(vid: &str, definition: u32, len: usize) {
        let c = crate::service::play_service::online::cache();
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

    // ---- 播放数据面：<video> 实际会发的几类请求 ----

    #[test]
    fn serves_full_body_after_fill() {
        ready_stream("v-full", 1080, 4096);
        let (status, headers, body) = serve("v-full", 1080, None).expect("整文件请求应成功");
        assert_eq!(status, 200);
        assert_eq!(body.len(), 4096);
        assert_eq!(header(&headers, "Content-Type"), Some("video/mp4"));
        assert_eq!(header(&headers, "Accept-Ranges"), Some("bytes"));
        assert_eq!(header(&headers, "Content-Length"), Some("4096"));
        // 不许 WebView2 缓存：缓存到失败瞬间的坏响应，重试就永远拿坏数据
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
        // 开放式 Range 是 <video> 起播的第一枪，必须给 206 + Content-Range。
        // 而且要一次给到文件末尾：整集早已在内存里，分段只会逼 <video> 反复
        // 续取，seek 时请求乱序，WebView2 会直接报错。
        ready_stream("v-open", 1080, 4096);
        let (status, headers, body) = serve("v-open", 1080, Some("bytes=0-")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body.len(), 4096);
        assert_eq!(header(&headers, "Content-Range"), Some("bytes 0-4095/4096"));
        assert_eq!(header(&headers, "Content-Length"), Some("4096"));
    }

    #[test]
    fn open_range_after_a_seek_also_reaches_the_end() {
        // 切清晰度会按续播位置 seek，`bytes=X-` 落在文件中段。
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
        // 等待中的条目被逐出/同键重建后，手里那份永远不会有数据（它的填充
        // 已随逐出死亡）——必须立即报错让前端自动重试接手，而不是等满
        // 30s 才把断流交给用户（「视频处理失败」那次事故的收尾半段）。
        let c = crate::service::play_service::online::cache();
        c.remove("v-stale", 1080);
        // 建一个空条目（无明文、无渐进）：serve 会进等待
        c.set_definitions("v-stale", 1080, &[]);
        let handle = std::thread::spawn(|| serve("v-stale", 1080, Some("bytes=0-")));
        std::thread::sleep(Duration::from_millis(300));
        // 同键重建 = 「切走又切回触发的重取」竞态
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

    // ---- 清晰度切换：一集的多个档位互不干扰 ----

    #[test]
    fn switching_quality_leaves_the_previous_one_serving_its_own_bytes() {
        // 「切档后报视频处理失败」的那个 bug：切档曾经会清空同一条缓存，
        // 于是正在播的旧 <video> 下一发 Range 请求就拿不到自己的数据。
        // 按 (vid, 档位) 分条目之后，切档对上一档是零影响。
        const FILL_1080: u8 = 0xA1;
        const FILL_720: u8 = 0xB2;
        let c = crate::service::play_service::online::cache();
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
        // 切到还没下过的档位：不能退而求其次拿别的档位的数据糊弄 `<video>`，
        // 那正是「容器索引对不上 → 解码失败」的来源。
        ready_stream("v-only-1080", 1080, 256);
        let err = serve("v-only-1080", 720, Some("bytes=0-")).expect_err("没下过的档位应报错");
        assert!(err.contains("没有正在准备的在线流"), "实际: {err}");
    }

    // ---------------------------------------------------------------- 渐进模式

    /// 注册一条带真实解密计划的渐进流，返回 (稀疏缓冲, 密文, 明文参照, 明文总长)。
    fn progressive_fixture(vid: &str, def: u32) -> (Arc<SparseBuffer>, Vec<u8>, Vec<u8>, u64) {
        use crate::domain::mp4::decrypt_buffer::decrypt_mp4_buffer;
        use crate::domain::mp4::streaming::StreamingPlan;
        use crate::domain::mp4::streaming_tests::assemble;

        let (_plain, cipher, key) = assemble(true);
        let reference = decrypt_mp4_buffer(&cipher, &key).expect("参照明文应可得");
        let len = cipher.len() as u64;
        let (abs, size) =
            crate::domain::mp4::streaming::locate_moov(&cipher, 0, len).expect("应定位 moov");
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
        let c = crate::service::play_service::online::cache();
        c.remove(vid, def);
        // 渐进注册是非复活式的：fixture 先物化条目（prepare 的 begin_fetch 等价物）
        assert!(c.begin_fetch(vid, def));
        c.end_fetch(vid, def);
        assert!(c.set_progressive(vid, def, Arc::new(prog)));
        (sparse, cipher, reference, plain_len)
    }

    #[test]
    fn progressive_serves_decrypted_range_as_data_arrives() {
        let (sparse, cipher, reference, plain_len) = progressive_fixture("v-prog", 720);

        // 数据未到时后台写入，serve 应等到就绪再给出**解密后**的字节
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
        // fixture 很小（<4MB 窗口），开放式给到明文末尾
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
        // 未加密流（plan = None）：明文就是密文，原样供数
        let c = crate::service::play_service::online::cache();
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

        // 还在等的时候：应有读者登记，且第一个需求洞指向缺口的起点
        let c = crate::service::play_service::online::cache();
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
        // 应答完成后登记销掉：残留的登记会占住调度优先级
        assert!(!prog.demand.has_waiters(), "应答完成后登记应注销");
    }

    #[test]
    fn progressive_is_reported_ready_before_any_data() {
        // 计划注册即可应答：拉分集与起播两个并发请求不必等整集
        let c = crate::service::play_service::online::cache();
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
        // 条目被逐出 = 用户已切走：渐进注册必须作废（否则填充把没人要的
        // 条目重新插回缓存，白占串行队列——切集黑屏链路的一环）
        let c = crate::service::play_service::online::cache();
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
        // 整集数据已写入后，渐进注册应被拒绝（回落路径赢了的情形）
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

    // ------------------------------------------------------------- 需求登记表

    #[test]
    fn reader_demand_serves_fifo_and_clears_on_unregister() {
        let d = ReaderDemand::new();
        let sparse = SparseBuffer::new(100);
        sparse.write(0, &[0u8; 10]); // 0-10 就绪

        let t1 = d.register(&[(0, 10)]); // 已就绪的区间
        let t2 = d.register(&[(80, 90)]); // 后到的、未就绪
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
        // 需求区间头尾就绪、中段有洞：调度器应指到区间内的洞，而不是越过它
        let d = ReaderDemand::new();
        let sparse = SparseBuffer::new(100);
        sparse.write(10, &[0u8; 10]); // 10-20
        sparse.write(25, &[0u8; 5]); // 25-30
        d.register(&[(10, 40)]);
        assert_eq!(d.first_wanted(&sparse), Some(20));
        sparse.write(20, &[0u8; 5]); // 20-25，区间前半补齐
        assert_eq!(
            d.first_wanted(&sparse),
            Some(30),
            "补齐后应指到区间内下一个洞"
        );
    }

    #[test]
    fn reader_demand_fifo_never_starves_early_reader() {
        // 先到的读者没就绪前，后到者不能插队（多读者并发的公平性）
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
