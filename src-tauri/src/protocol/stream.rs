//! 在线播放的内存流（`hongguo-stream://`）。
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
//!   按明文 Range 等齐依赖的密文区间、惰性解密拼装（见 [`serve_progressive`]）。
//!   首帧只需头 + 尾 + moov 就位。
//! - **整集**：明文一次性原子写进 `store`，`size` 从 0 变成真实值的那一刻起
//!   全部字节就都在内存里，任意 Range 直接切。渐进路径不可用（CDN 不支持
//!   Range 等）时的回落。
//!
//! 渐进模式的开放式 Range 必须开窗口（默认 4MB，`HONGGUO_STREAM_WINDOW`
//! 可覆盖）：给到底就等于要等整集，渐进失去意义。整集模式保持「一次给到底」。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::range::{parse_range, partial_headers, ProtocolResponse, RangeSpec};
use crate::domain::model::VideoDefinition;
use crate::domain::mp4::streaming::{SparseBuffer, StreamingPlan};

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
pub struct ProgressiveStream {
    pub sparse: Arc<SparseBuffer>,
    pub plan: Option<Arc<StreamingPlan>>,
    /// 明文总长（`plan` 为 `None` 时等于密文总长）
    pub plain_len: u64,
    /// serve 遇到未覆盖区间时置位，取流线程据此跳转填充（明文偏移）
    pub seek_hint: AtomicU64,
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

    // 两条路：整集模式（数据一次性原子写进 buffer）等 filled；
    // 渐进模式等计划注册（plain_len 已知即可应答，字节按区间再等）。
    // 取流失败回落整集时，两条都可能出现，谁先就绪走谁。
    let mode = match wait_until(Duration::from_secs(WAIT_TIMEOUT_SECS), || {
        let prog = entry.progressive.lock().clone();
        if *entry.filled.lock() > 0 {
            Some(None)
        } else {
            prog.map(Some)
        }
    }) {
        Some(m) => m,
        None => {
            log::warn!("[Stream] {vid}@{definition} 等待流就绪超时（{WAIT_TIMEOUT_SECS}s）");
            return Err("等待流就绪超时".to_string());
        }
    };

    match mode {
        // 整集模式：size 只在 store 里从 0 变成真实值一次，之后再不变
        None => {
            let size = *entry.size.lock();
            let buffer = entry.buffer.lock();
            Ok(respond(&buffer, parse_range(range_header, size), size))
        }
        Some(prog) => serve_progressive(&prog, range_header),
    }}

/// 渐进模式应答：把明文 Range 映射回密文区间，等齐、解密、拼装。
fn serve_progressive(
    prog: &Arc<ProgressiveStream>,
    range_header: Option<&str>,
) -> Result<ProtocolResponse, String> {
    let plain_len = prog.plain_len;
    let (start, end) = match parse_range(range_header, plain_len) {
        RangeSpec::Full => (0, plain_len),
        RangeSpec::Closed { start, end } => (start, end + 1),
        RangeSpec::Open { start } => {
            let window = progressive_window();
            (start, (start + window).min(plain_len))
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

    // 等依赖的密文区间。等之前给取流线程留 seek 提示：开放式/闭式 Range
    // 落在尚未下载的前方时，顺序填充按提示跳过去，用户不用干等到下完
    let needed = prog.needed(start, end);
    if !needed.iter().all(|(a, b)| prog.sparse.covers(*a, *b)) {
        if let Some((first, _)) = needed.iter().find(|(a, b)| !prog.sparse.covers(*a, *b)) {
            prog.seek_hint.fetch_max(*first, Ordering::Release);
        }
    }
    if !prog.sparse.wait_cover(&needed, Duration::from_secs(WAIT_TIMEOUT_SECS)) {
        // 典型成因：填充线程停摆/被逐出后重填未到，播放追上了填充前沿
        log::warn!(
            "[Stream] 等待明文区间 {start}-{end} 的密文就绪超时（{WAIT_TIMEOUT_SECS}s），填充可能停摆"
        );
        return Err("等待流就绪超时".to_string());
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
            ],
            body,
        ))
    }
}

/// 渐进模式的开放 Range 窗口大小，可用 `HONGGUO_STREAM_WINDOW` 覆盖。
fn progressive_window() -> u64 {
    static WINDOW: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *WINDOW.get_or_init(|| {
        std::env::var("HONGGUO_STREAM_WINDOW")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(PROGRESSIVE_WINDOW)
    })
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

/// 轮询等待条件成立并取回结果，超时返回 `None`。
///
/// 协议 handler 跑在专用工作线程上，轮询足够且简单可靠
/// （比在协议线程里架一套 tokio runtime 更不容易出死锁）。
fn wait_until<T>(timeout: std::time::Duration, mut cond: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(v) = cond() {
            return Some(v);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// 按 Range 切一段数据出来（整集模式）。
///
/// 只在 `size` 已经就绪之后调用，而那意味着整集明文全部在内存里了
/// （见 [`StreamCache::store`]），所以每一种 Range 都一定切得出来，没有「等数据」。
fn respond(buffer: &[u8], range: RangeSpec, size: u64) -> ProtocolResponse {
    match range {
        RangeSpec::Full => (
            200,
            vec![
                ("Content-Type".into(), "video/mp4".into()),
                ("Accept-Ranges".into(), "bytes".into()),
                ("Content-Length".into(), size.to_string()),
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

/// 一档正在准备的流。
#[derive(Default)]
struct StreamEntry {
    /// 已解密的明文缓冲（整集模式）
    buffer: Mutex<Vec<u8>>,
    /// 已填充到的位置（字节数）
    filled: Mutex<u64>,
    /// 总大小
    size: Mutex<u64>,
    /// 渐进模式的数据面。注册即代表 plain_len 已知、协议可开始应答
    progressive: Mutex<Option<Arc<ProgressiveStream>>>,
    /// 本集提供的全部档位，供播放菜单读取
    definitions: Mutex<Vec<VideoDefinition>>,
    /// 是否正在取流（接口已发出、数据尚未落盘）。
    ///
    /// 播放页会对同一集发两次 `play_series`（拉分集 + 起播），两次都去下载
    /// 就是白下一整集，所以第二个要转为等待而不是重复发起。
    fetching: Mutex<bool>,
}

/// 缓存条目：同一集的不同清晰度各占一条。
#[derive(PartialEq, Eq, Hash)]
struct StreamKey {
    vid: String,
    definition: u32,
}

/// 全部在线流的注册表。
#[derive(Default)]
pub struct StreamCache {
    entries: Mutex<HashMap<StreamKey, Arc<StreamEntry>>>,
    /// 「不指定档位」时上一轮解析到的档位。
    ///
    /// 缓存按 (vid, 档位) 分条目，不指定档位就没有键可查。记下上一轮的答案，
    /// 下一个不带档位的请求就能直接命中，不必再发一次接口调用。
    auto: Mutex<HashMap<String, u32>>,
}

impl StreamCache {
    /// 取或创建一个条目。
    fn get_or_create(&self, vid: &str, definition: u32) -> Arc<StreamEntry> {
        let key = StreamKey {
            vid: vid.to_string(),
            definition,
        };
        let mut guard = self.entries.lock();
        guard
            .entry(key)
            .or_insert_with(|| Arc::new(StreamEntry::default()))
            .clone()
    }

    /// 查一个条目。
    fn get(&self, vid: &str, definition: u32) -> Option<Arc<StreamEntry>> {
        self.entries
            .lock()
            .get(&StreamKey {
                vid: vid.to_string(),
                definition,
            })
            .cloned()
    }

    /// 这一档的条目是否还在（取流失败会被删掉）。
    pub fn exists(&self, vid: &str, definition: u32) -> bool {
        self.get(vid, definition).is_some()
    }

    /// 这一档是否已经可以供 `<video>` 取数；可以的话带回它提供的全部档位。
    ///
    /// 渐进模式注册了计划就算就绪：首帧等的是数据区间，不是整集。
    pub fn ready(&self, vid: &str, definition: u32) -> Option<Vec<VideoDefinition>> {
        let entry = self.get(vid, definition)?;
        if *entry.filled.lock() == 0 && entry.progressive.lock().is_none() {
            return None;
        }
        let definitions = entry.definitions.lock().clone();
        Some(definitions)
    }

    /// 注册渐进式流（取流编排建好计划后调用）。
    ///
    /// 整集数据已经写进来的话返回 `false`（渐进条目作废），调用方据此停止填充。
    pub fn set_progressive(&self, vid: &str, definition: u32, stream: Arc<ProgressiveStream>) -> bool {
        let entry = self.get_or_create(vid, definition);
        let filled = entry.filled.lock();
        if *filled > 0 {
            return false;
        }
        *entry.progressive.lock() = Some(stream);
        true
    }

    /// 写入整段明文。已经有数据时**不覆盖**。
    ///
    /// 返回 `false` 表示这份数据作废。取流可能重复发生（拉分集与起播两个
    /// 请求会各下一遍），而按 `filled` 往后追加的话，同一份数据写两遍就会
    /// 把 buffer 拼成两段。
    pub fn store(&self, vid: &str, definition: u32, bytes: &[u8]) -> bool {
        let entry = self.get_or_create(vid, definition);
        let mut buffer = entry.buffer.lock();
        let mut filled = entry.filled.lock();
        let mut size = entry.size.lock();
        if *filled > 0 {
            return false;
        }
        buffer.extend_from_slice(bytes);
        *filled = bytes.len() as u64;
        *size = bytes.len() as u64;
        true
    }

    /// 记下本集提供的全部档位。
    pub fn set_definitions(&self, vid: &str, definition: u32, definitions: &[VideoDefinition]) {
        *self.get_or_create(vid, definition).definitions.lock() = definitions.to_vec();
    }

    /// 标记「正在取流」。返回 `true` 表示本次调用抢到了取流权。
    pub fn begin_fetch(&self, vid: &str, definition: u32) -> bool {
        let entry = self.get_or_create(vid, definition);
        let mut g = entry.fetching.lock();
        if *g {
            false
        } else {
            *g = true;
            true
        }
    }

    /// 取流结束（无论成功失败），放开门。
    pub fn end_fetch(&self, vid: &str, definition: u32) {
        if let Some(entry) = self.get(vid, definition) {
            *entry.fetching.lock() = false;
        }
    }

    /// 这一档是否正在取流。
    pub fn is_fetching(&self, vid: &str, definition: u32) -> bool {
        self.get(vid, definition)
            .is_some_and(|e| *e.fetching.lock())
    }

    /// 丢弃一档（取流失败时调用）。
    pub fn remove(&self, vid: &str, definition: u32) {
        self.entries.lock().remove(&StreamKey {
            vid: vid.to_string(),
            definition,
        });
    }

    /// 同「只留一集」语义，但**正在取流的条目豁免**。
    ///
    /// 沉浸流会预取下一部剧的流（后台渐进填充中）；用户此时切集/换清晰度
    /// 触发新的 `prepare`，无脑清理会把预取半途的条目清掉，
    /// 等于白取一遍。豁免后预取能活到它被切上的那一刻。
    pub fn keep_only_protect_fetching(&self, vid: &str) {
        self.entries
            .lock()
            .retain(|k, entry| k.vid == vid || *entry.fetching.lock());
    }

    /// 这一集「不指定档位」时上次解析到的档位。
    pub fn auto_definition(&self, vid: &str) -> Option<u32> {
        self.auto.lock().get(vid).copied()
    }

    /// 记下这次实际生效的档位，供下一次「不指定档位」的请求直接命中。
    pub fn remember_auto(&self, vid: &str, definition: u32) {
        self.auto.lock().insert(vid.to_string(), definition);
    }

    /// 缓存状态：(条目数, 已缓存字节)。
    pub fn status(&self) -> (usize, u64) {
        let guard = self.entries.lock();
        let bytes: u64 = guard.values().map(|e| *e.filled.lock()).sum();
        (guard.len(), bytes)
    }

    /// 清空。
    pub fn clear(&self) {
        self.entries.lock().clear();
        self.auto.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn wait_until_returns_the_value_not_just_a_flag() {
        let mut polls = 0;
        let got = wait_until(Duration::from_secs(5), || {
            polls += 1;
            (polls >= 2).then_some(42u32)
        });
        assert_eq!(got, Some(42));
    }

    #[test]
    fn wait_until_gives_up_at_the_deadline() {
        assert_eq!(wait_until(Duration::ZERO, || None::<u32>), None);
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

    #[test]
    fn store_keeps_the_first_result_and_rejects_a_duplicate() {
        let c = StreamCache::default();
        assert!(c.store("v1", 1080, &[1u8; 8]), "第一次应写入");
        assert!(!c.store("v1", 1080, &[2u8; 8]), "重复的那份应作废");
        let entry = c.get("v1", 1080).expect("条目应存在");
        assert_eq!(*entry.filled.lock(), 8, "不能被拼成两段");
        assert!(entry.buffer.lock().iter().all(|b| *b == 1));
    }

    #[test]
    fn ready_only_reports_a_fully_filled_entry() {
        let c = StreamCache::default();
        let def = VideoDefinition {
            value: 1080,
            width: 1920,
            height: 1080,
        };
        c.set_definitions("v2", 1080, &[def]);
        assert!(
            c.ready("v2", 1080).is_none(),
            "只有档位表、没有数据时不算就绪"
        );
        c.store("v2", 1080, &[7u8; 4]);
        assert_eq!(c.ready("v2", 1080), Some(vec![def]));
    }

    #[test]
    fn begin_fetch_grants_exactly_one_holder() {
        // 播放页对同一集会并发发两次 play_series：只有一次能拿到取流权，
        // 另一次要转为等待，否则整集会被白下一遍。
        let c = StreamCache::default();
        assert!(c.begin_fetch("v1", 1080), "第一次应拿到取流权");
        assert!(!c.begin_fetch("v1", 1080), "第二次必须被拒");
        assert!(c.is_fetching("v1", 1080));
        c.end_fetch("v1", 1080);
        assert!(!c.is_fetching("v1", 1080));
        assert!(c.begin_fetch("v1", 1080), "放开后应能重新拿到");
    }

    #[test]
    fn fetching_is_tracked_per_stream() {
        let c = StreamCache::default();
        c.begin_fetch("a", 1080);
        assert!(c.is_fetching("a", 1080));
        assert!(!c.is_fetching("a", 720), "别的档位不受影响");
        assert!(!c.is_fetching("b", 1080), "别的 vid 不受影响");
    }

    #[test]
    fn keep_only_drops_other_episodes_but_spares_fetching() {
        // 一次只看一集：不清掉别的集，整季 250 集会把内存吃光；
        // 正在取流的豁免（沉浸流预取的下一部剧不能被清）
        let c = StreamCache::default();
        c.store("ep1", 1080, &[0u8; 100]);
        c.store("ep2", 1080, &[0u8; 50]);
        c.begin_fetch("ep3", 1080);
        c.keep_only_protect_fetching("ep2");
        assert!(c.get("ep1", 1080).is_none());
        assert!(c.get("ep2", 1080).is_some());
        assert!(c.get("ep3", 1080).is_some(), "取流中的预取条目豁免");
        c.end_fetch("ep3", 1080);
        c.keep_only_protect_fetching("ep2");
        assert!(c.get("ep3", 1080).is_none(), "取流结束后不再豁免");
    }

    #[test]
    fn auto_definition_lets_the_next_untargeted_request_hit_the_cache() {
        let c = StreamCache::default();
        assert_eq!(c.auto_definition("v1"), None);
        c.remember_auto("v1", 1080);
        assert_eq!(c.auto_definition("v1"), Some(1080));
    }

    #[test]
    fn remove_clears_the_fetch_flag_with_the_entry() {
        // 失败路径会 remove：留下的 fetching 会让后续请求永远走等待分支
        let c = StreamCache::default();
        c.begin_fetch("v1", 1080);
        c.remove("v1", 1080);
        assert!(!c.is_fetching("v1", 1080));
        assert!(c.get("v1", 1080).is_none());
    }

    #[test]
    fn status_counts_entries_and_bytes() {
        let c = StreamCache::default();
        c.store("a", 1080, &[0u8; 100]);
        c.store("b", 1080, &[0u8; 50]);
        let (count, bytes) = c.status();
        assert_eq!(count, 2);
        assert_eq!(bytes, 150);
    }

    #[test]
    fn clear_removes_entries_and_remembered_definitions() {
        let c = StreamCache::default();
        c.store("a", 1080, &[0u8; 10]);
        c.remember_auto("a", 1080);
        c.clear();
        assert_eq!(c.status().0, 0);
        assert_eq!(c.auto_definition("a"), None);
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
            seek_hint: AtomicU64::new(0),
        };
        let c = crate::service::play_service::online::cache();
        c.remove(vid, def);
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
            seek_hint: AtomicU64::new(0),
        };
        c.remove("v-prog-raw", 720);
        assert!(c.set_progressive("v-prog-raw", 720, Arc::new(prog)));

        let (status, _, body) = serve("v-prog-raw", 720, Some("bytes=100-199")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body, &data[100..200]);
    }

    #[test]
    fn progressive_leave_seek_hint_while_waiting() {
        let (sparse, cipher, _reference, _len) = progressive_fixture("v-prog-hint", 720);

        let server = std::thread::spawn(move || {
            let r = serve("v-prog-hint", 720, Some("bytes=10-19"));
            (r.is_ok(), r.map(|(_, _, b)| b.len()).unwrap_or(0))
        });
        std::thread::sleep(Duration::from_millis(150));

        // 还在等的时候，seek 提示应指向缺口的起点
        let c = crate::service::play_service::online::cache();
        let hint = c
            .get("v-prog-hint", 720)
            .and_then(|e| e.progressive.lock().clone())
            .map(|p| p.seek_hint.load(Ordering::Acquire))
            .unwrap_or(0);
        assert_eq!(hint, 10, "等待期间应留下 seek 提示");

        sparse.write(0, &cipher);
        let (ok, len) = server.join().unwrap();
        assert!(ok);
        assert_eq!(len, 10);
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
            seek_hint: AtomicU64::new(0),
        };
        assert!(c.set_progressive("v-prog-ready", 720, Arc::new(prog)));
        assert!(c.ready("v-prog-ready", 720).is_some(), "注册即就绪");
    }

    #[test]
    fn set_progressive_yields_to_whole_mode_data() {
        // 整集数据已写入后，渐进注册应被拒绝（回落路径赢了的情形）
        let c = StreamCache::default();
        c.store("v-race", 720, &[1u8; 8]);
        let sparse = SparseBuffer::new(4);
        let prog = ProgressiveStream {
            sparse,
            plan: None,
            plain_len: 4,
            seek_hint: AtomicU64::new(0),
        };
        assert!(!c.set_progressive("v-race", 720, Arc::new(prog)));
    }
}
