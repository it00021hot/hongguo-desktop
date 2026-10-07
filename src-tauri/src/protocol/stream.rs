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
                if let Some((gap, _)) = sparse.next_gap(*a) {
                    if gap < *b {
                        return Some(gap);
                    }
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

/// 一档正在准备的流。
pub struct StreamEntry {
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
    /// 最近一次被用到的时刻（LRU 指纹）。`get`/创建即触碰：
    /// 协议层每次 Range 请求都会读条目，这正是「这条流还在被播放器读」
    /// 的最直接证据，逐出策略据此留人。
    last_used: Mutex<std::time::Instant>,
}

impl Default for StreamEntry {
    fn default() -> Self {
        Self {
            buffer: Mutex::new(Vec::new()),
            filled: Mutex::new(0),
            size: Mutex::new(0),
            progressive: Mutex::new(None),
            definitions: Mutex::new(Vec::new()),
            fetching: Mutex::new(false),
            last_used: Mutex::new(std::time::Instant::now()),
        }
    }
}

/// 条目的内存占位（字节）：渐进流按明文总长计，整集按落盘明文计。
fn entry_weight(e: &StreamEntry) -> u64 {
    if let Some(p) = &*e.progressive.lock() {
        p.plain_len
    } else {
        *e.size.lock()
    }
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
    /// 当前正在看的 vid。prepare 时设置；逐出策略只保它 + 显式预取 + 最近几条。
    current: Mutex<String>,
    /// 显式预取的档位（沉浸流「下一部剧」），有界（[`MAX_PREFETCH_MARKS`]），
    /// 新标记挤掉最老的。逐出策略额外保这些条目；填充礼让也认它。
    prefetch: Mutex<Vec<(String, u32)>>,
}

/// 除当前集与显式预取外，再保留几条最近用过的流。
///
/// 信息流切剧的目标会在毫秒级来回弹（快速滚动、tab 连点），挂载在
/// `<video>` 上的流永远比「当前目标」旧一拍到两拍——只保当前集的旧策略
/// 会把正在被读的流逐掉，播放追上缓冲前沿就是 30s 超时 + MediaError
/// （2026-10-06「视频处理失败」的根因）。多留几条，弹动就打不穿缓存。
const KEEP_RECENT: usize = 3;

/// 预取标记的上限。首页同一时刻最多预取一部，快速连滚会短暂多出一两
/// 个标记，上限取 2 足够，防止标记无界增长把逐出豁免变成内存漏洞。
const MAX_PREFETCH_MARKS: usize = 2;

/// 在线流缓存的总字节预算。当前集 + 预取 + 最近保留加起来超过它时，
/// 从最旧的「非当前」条目开始逐出，直到回到预算内。
const RETAIN_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

impl StreamCache {
    /// 取或创建一个条目。
    fn get_or_create(&self, vid: &str, definition: u32) -> Arc<StreamEntry> {
        let key = StreamKey {
            vid: vid.to_string(),
            definition,
        };
        let entry = self
            .entries
            .lock()
            .entry(key)
            .or_insert_with(|| Arc::new(StreamEntry::default()))
            .clone();
        *entry.last_used.lock() = std::time::Instant::now();
        entry
    }

    /// 查一个条目。读到即触碰（LRU）：协议层的 Range 请求都走这里，
    /// 「还在被播放器读」由这条路径自然记账。
    pub fn get(&self, vid: &str, definition: u32) -> Option<Arc<StreamEntry>> {
        let entry = self
            .entries
            .lock()
            .get(&StreamKey {
                vid: vid.to_string(),
                definition,
            })
            .cloned();
        if let Some(e) = &entry {
            *e.last_used.lock() = std::time::Instant::now();
        }
        entry
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
    /// 条目已被逐出（用户切走了）同样返回 `false` 且**不再重建**——重建一个
    /// 没人要的条目只会让它的填充白占串行队列。
    pub fn set_progressive(
        &self,
        vid: &str,
        definition: u32,
        stream: Arc<ProgressiveStream>,
    ) -> bool {
        let Some(entry) = self.get(vid, definition) else {
            log::info!("[Stream] {vid}@{definition} 已被逐出，渐进注册作废");
            return false;
        };
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
        // 失败的预取不留标记：留着会让下一条预取被挤出界，还豁免一个不存在的条目
        self.prefetch
            .lock()
            .retain(|(v, d)| v != vid || *d != definition);
    }

    /// 逐出策略（2026-10-06 二代）：换当前集时保留——
    ///
    /// 1. 当前 vid 的全部档位；
    /// 2. 显式预取的条目（有界标记）；
    /// 3. 最近用过的至多 [`KEEP_RECENT`] 条其它条目（LRU 指纹来自 `get`）；
    ///
    /// 之后按总字节预算（[`RETAIN_BUDGET_BYTES`]）从最旧的非当前条目继续逐。
    ///
    /// 为什么不只保当前集：信息流切剧的目标会毫秒级来回弹，挂载在
    /// `<video>` 上的流永远比「当前目标」旧一到两拍——把它逐掉，播放
    /// 追上缓冲前沿就是 30s 超时 + MediaError。但也不再 blanket 豁免全部
    /// fetching：被切走太久、已经落到保留窗外的填充仍会据 `exists` 自行
    /// 中止，串行队列照样立刻让给当前集。
    pub fn retain_playing_set(&self, vid: &str) {
        self.retain_with(vid, KEEP_RECENT, RETAIN_BUDGET_BYTES);
    }

    /// [`Self::retain_playing_set`] 的可注入形态（测试用）。
    fn retain_with(&self, vid: &str, keep_recent: usize, budget: u64) {
        let prefetch = self.prefetch.lock().clone();
        let mut entries = self.entries.lock();
        let is_marked = |k: &StreamKey| {
            prefetch
                .iter()
                .any(|(v, d)| v == &k.vid && *d == k.definition)
        };

        // 候选逐出集：非当前、非预取。按最近使用降序，窗口外即逐出。
        let mut others: Vec<(StreamKey, std::time::Instant, u64)> = entries
            .iter()
            .filter(|(k, _)| k.vid != vid && !is_marked(k))
            .map(|(k, e)| {
                (
                    StreamKey {
                        vid: k.vid.clone(),
                        definition: k.definition,
                    },
                    *e.last_used.lock(),
                    entry_weight(e),
                )
            })
            .collect();
        others.sort_by_key(|(_, last_used, _)| std::cmp::Reverse(*last_used));
        for (k, _, _) in others.iter().skip(keep_recent) {
            entries.remove(k);
        }

        // 字节预算：从最旧的非当前条目开始逐，直到回到预算内。
        // 预算优先于条数：刚被窗口留下的条目一样可能因预算出局。
        let mut total: u64 = entries.values().map(|e| entry_weight(e)).sum();
        if total <= budget {
            return;
        }
        others.sort_by_key(|(_, last_used, _)| *last_used);
        for (k, _, weight) in others {
            if total <= budget {
                break;
            }
            if entries.remove(&k).is_some() {
                total = total.saturating_sub(weight);
            }
        }
    }

    /// 记下当前正在看的 vid（prepare 时调用；逐出策略与填充礼让据此判断）。
    pub fn set_current(&self, vid: &str) {
        *self.current.lock() = vid.to_string();
        // 升格为当前的那(几)条预取不再是预取
        self.prefetch.lock().retain(|(v, _)| v != vid);
    }

    /// 这一条是否还在预取标记里（填充让位判定用）。
    pub fn is_prefetch_marked(&self, vid: &str, definition: u32) -> bool {
        self.prefetch
            .lock()
            .iter()
            .any(|(v, d)| v == vid && *d == definition)
    }

    /// 标记一条为预取（有界：超出 [`MAX_PREFETCH_MARKS`] 挤掉最老的）。
    pub fn mark_prefetch(&self, vid: &str, definition: u32) {
        let mut pf = self.prefetch.lock();
        pf.retain(|(v, d)| v != vid || *d != definition);
        pf.push((vid.to_string(), definition));
        while pf.len() > MAX_PREFETCH_MARKS {
            pf.remove(0);
        }
    }

    /// 当前正在看的 vid。
    pub fn current(&self) -> String {
        self.current.lock().clone()
    }

    /// 同 vid 任一档位已就绪就带回 `(实际档位, 全部档位表)`。
    ///
    /// 「不指定档位」又没有 auto 记录时的兜底命中：条目键里必须带具体档位，
    /// 查不到键不等于没有现成的流。宁可复用别的档位，也好过把取流表接口、
    /// 建流、填头部整条链路再走一遍——那正是切剧黑屏待在身上的时间。
    pub fn ready_any(&self, vid: &str) -> Option<(u32, Vec<VideoDefinition>)> {
        let entries = self.entries.lock();
        entries
            .iter()
            .filter(|(k, _)| k.vid == vid)
            .find(|(_, e)| *e.filled.lock() > 0 || e.progressive.lock().is_some())
            .map(|(k, e)| (k.definition, e.definitions.lock().clone()))
    }

    /// 预取条目是否需要「转正续填」：渐进已注册但远没填满，且没有填充方。
    pub fn needs_resume(&self, vid: &str, definition: u32) -> bool {
        let Some(entry) = self.get(vid, definition) else {
            return false;
        };
        if *entry.fetching.lock() {
            return false;
        }
        let Some(prog) = entry.progressive.lock().clone() else {
            return false;
        };
        prog.sparse.downloaded() < prog.sparse.len()
    }

    /// 取续填所需的数据面（稀疏缓冲 + 渐进计划）。仅 [`Self::needs_resume`]
    /// 为真时有值；续填的编排（取流权/事件上报）在 play_service 侧。
    pub fn resume_parts(
        &self,
        vid: &str,
        definition: u32,
    ) -> Option<(Arc<SparseBuffer>, Arc<ProgressiveStream>)> {
        let entry = self.get(vid, definition)?;
        let prog = entry.progressive.lock().clone()?;
        Some((prog.sparse.clone(), prog))
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
        self.prefetch.lock().clear();
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

    /// 免触碰的存在性检查：`get` 会刷新 LRU 指纹，断言里用它才不打乱时序。
    fn present(c: &StreamCache, vid: &str, definition: u32) -> bool {
        c.entries.lock().contains_key(&StreamKey {
            vid: vid.to_string(),
            definition,
        })
    }

    #[test]
    fn retain_keeps_current_marks_and_a_few_recent_others() {
        // 换当前集不再「只留一集」：信息流切剧目标会毫秒级来回弹，
        // 正在被 <video> 读的流必须还活着。保留 = 当前集 + 显式预取 +
        // 最近 KEEP_RECENT 条；窗口外的照样逐（填充据 exists 自行中止，
        // 串行队列不会被打断的填充堵死）。
        let c = StreamCache::default();
        c.store("cur", 1080, &[0u8; 10]);
        c.store("marked", 1080, &[0u8; 10]);
        c.mark_prefetch("marked", 1080);
        // o1 最旧 … o4 最新（Instant 分辨率可能高于睡眠时长，逐个拉开）
        for v in ["o1", "o2", "o3", "o4"] {
            c.store(v, 1080, &[0u8; 10]);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        c.retain_playing_set("cur");
        assert!(present(&c, "cur", 1080), "当前集必留");
        assert!(present(&c, "marked", 1080), "显式预取必留");
        assert!(present(&c, "o4", 1080), "最近的在保留窗内");
        assert!(present(&c, "o3", 1080), "最近的在保留窗内");
        assert!(present(&c, "o2", 1080), "最近的在保留窗内");
        assert!(!present(&c, "o1", 1080), "最旧的被逐出");

        // 换当前集后，旧当前集降级为「最近用过」，在窗口内不会被新目标清掉
        c.set_current("o4");
        c.retain_playing_set("o4");
        assert!(
            present(&c, "cur", 1080),
            "刚切走的上一部仍在保留窗内（挂载的 <video> 可能还在读它）"
        );
    }

    #[test]
    fn retain_window_evicts_oldest_beyond_keep_recent() {
        let c = StreamCache::default();
        c.store("big-cur", 1080, &[0u8; 10]);
        for i in 0..6 {
            c.store(&format!("f{i}"), 1080, &[0u8; 10]);
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        c.retain_playing_set("big-cur");
        assert!(present(&c, "big-cur", 1080), "当前集必留");
        // 窗口 3 条：最旧的 f0、f1、f2 出局，f3、f4、f5 留下
        assert!(!present(&c, "f0", 1080));
        assert!(!present(&c, "f1", 1080));
        assert!(!present(&c, "f2", 1080));
        assert!(present(&c, "f3", 1080));
        assert!(present(&c, "f4", 1080));
        assert!(present(&c, "f5", 1080));
    }

    #[test]
    fn retain_budget_evicts_oldest_first_until_under_budget() {
        // 预算优先于条数：总字节超预算时按最旧优先逐，当前集豁免。
        let c = StreamCache::default();
        c.store("cur", 1080, &[0u8; 30]);
        for i in 0..4 {
            c.store(&format!("b{i}"), 1080, &[0u8; 10]);
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        // 总量 70 > 预算 50：逐最旧的 b0、b1（各 10B）后回到 50
        c.retain_with("cur", 4, 50);
        assert!(present(&c, "cur", 1080), "当前集即便最大也豁免");
        assert!(!present(&c, "b0", 1080));
        assert!(!present(&c, "b1", 1080));
        assert!(present(&c, "b2", 1080), "回到预算内就停手");
        assert!(present(&c, "b3", 1080));
    }

    #[test]
    fn prefetch_marks_are_bounded() {
        // 标记无界会把逐出豁免变成内存漏洞：只留最新 MAX_PREFETCH_MARKS 个。
        let c = StreamCache::default();
        c.mark_prefetch("p1", 1080);
        c.mark_prefetch("p2", 1080);
        c.mark_prefetch("p3", 1080);
        assert_eq!(c.prefetch.lock().len(), super::MAX_PREFETCH_MARKS);
        assert!(
            !c.prefetch.lock().iter().any(|(v, _)| v == "p1"),
            "最老的标记被挤出"
        );
        // 转正（set_current）与删除（remove）都要摘标记
        c.set_current("p3");
        assert!(c.prefetch.lock().iter().all(|(v, _)| v != "p3"));
        c.mark_prefetch("p9", 720);
        c.remove("p9", 720);
        assert!(c.prefetch.lock().iter().all(|(v, _)| v != "p9"));
    }

    #[test]
    fn ready_any_returns_any_ready_definition_of_the_same_vid() {
        let c = StreamCache::default();
        assert!(c.ready_any("v-any").is_none(), "没有条目时无值");
        c.set_definitions("v-any", 720, &[]);
        assert!(c.ready_any("v-any").is_none(), "只有档位表不算就绪");
        c.store("v-any", 720, &[0u8; 8]);
        let (def, _) = c.ready_any("v-any").expect("有明文条目就应命中");
        assert_eq!(def, 720);
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
