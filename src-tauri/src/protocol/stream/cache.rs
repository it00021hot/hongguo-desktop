//! 在线流缓存：`StreamCache` 全局 LRU——(vid, 档位) 分条目、保留窗与字节预算逐出。

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::progressive::ProgressiveStream;
use crate::domain::model::VideoDefinition;
use crate::domain::mp4::streaming::SparseBuffer;

/// 一档正在准备的流。
pub struct StreamEntry {
    /// 已解密的明文缓冲（整集模式）
    pub(super) buffer: Mutex<Vec<u8>>,
    /// 已填充到的位置（字节数）
    pub(super) filled: Mutex<u64>,
    /// 总大小
    pub(super) size: Mutex<u64>,
    /// 渐进模式的数据面。注册即代表 plain_len 已知、协议可开始应答
    pub(super) progressive: Mutex<Option<Arc<ProgressiveStream>>>,
    /// 本集提供的全部档位，供播放菜单读取
    pub(super) definitions: Mutex<Vec<VideoDefinition>>,
    /// 是否正在取流（接口已发出、数据尚未落盘）。
    ///
    /// 播放页会对同一集发两次 `play_series`（拉分集 + 起播），两次都去下载
    /// 就是白下一整集，所以第二个要转为等待而不是重复发起。
    pub(super) fetching: Mutex<bool>,
    /// 最近一次被用到的时刻（LRU 指纹）。`get`/创建即触碰：
    /// 协议层每次 Range 请求都会读条目，这正是「这条流还在被播放器读」
    /// 的最直接证据，逐出策略据此留人。
    pub(super) last_used: Mutex<std::time::Instant>,
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
}
