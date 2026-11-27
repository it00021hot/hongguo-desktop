//! 在线流缓存：`StreamCache` 全局 LRU——(vid, 档位) 分条目、保留窗与字节预算逐出。

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use super::progressive::ProgressiveStream;
use hongguo_common::model::VideoDefinition;
use hongguo_video::streaming::SparseBuffer;

/// 一档正在准备的流。
pub struct StreamEntry {
    /// 已解密的明文缓冲（整集模式）
    pub(crate) buffer: Mutex<Vec<u8>>,
    pub(crate) filled: Mutex<u64>,
    pub(crate) size: Mutex<u64>,
    /// 渐进模式的数据面；注册即代表 plain_len 已知、协议可开始应答
    pub(crate) progressive: Mutex<Option<Arc<ProgressiveStream>>>,
    /// 本集提供的全部档位，供播放菜单读取
    pub(crate) definitions: Mutex<Vec<VideoDefinition>>,
    /// 是否正在取流。播放页对同一集会发两次 `play_series`，第二个必须转等待而不是重复下载。
    pub(crate) fetching: Mutex<bool>,
    /// 最近一次被用到的时刻（LRU 指纹）：`get`/创建即触碰，逐出策略据此判断「这条流还在被播放器读」。
    pub(crate) last_used: Mutex<std::time::Instant>,
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

/// 条目的内存占位（字节）：渐进流 = 稀疏缓冲实际分配 + 解密计划常驻
///（`new_moov` + 样本段索引；漏算会让每集约数 MB 的计划变成 Status 对不上的 rss 残渣，
/// 预算逐出也会偏松）；整集模式按落盘明文计。
fn entry_weight(e: &StreamEntry) -> u64 {
    if let Some(p) = &*e.progressive.lock() {
        let plan = p.plan.as_ref().map(|pl| pl.footprint_bytes()).unwrap_or(0);
        p.sparse.allocated() + plan
    } else {
        *e.size.lock()
    }
}

/// 缓存条目：同一集的不同清晰度各占一条。
#[derive(Clone, PartialEq, Eq, Hash)]
struct StreamKey {
    vid: String,
    definition: u32,
}

/// The specific cache entry used to derive a media source's byte layout.
#[derive(Clone)]
pub struct StreamGeneration {
    key: StreamKey,
    entry: Weak<StreamEntry>,
}

/// 全部在线流的注册表。
#[derive(Default)]
pub struct StreamCache {
    entries: Mutex<HashMap<StreamKey, Arc<StreamEntry>>>,
    /// 当前正在看的 vid。prepare 时设置；逐出策略只保它 + 显式预取 + 最近几条。
    current: Mutex<String>,
    /// 上一任正在看的 vid：stageNext 双槽提前准备下一集时，当前集仍在播最后几秒，必须免于被预算逐出。
    previous_current: Mutex<String>,
    /// 显式预取的档位（沉浸流「下一部剧」），有界（`MAX_PREFETCH_MARKS`），
    /// 新标记挤掉最老的。逐出策略额外保这些条目；填充礼让也认它。
    prefetch: Mutex<Vec<(String, u32)>>,
}

/// 除当前集与显式预取外，再保留几条最近用过的流：信息流切剧的目标毫秒级来回弹，
/// 挂载在 `<video>` 上的流永远比「当前目标」旧一到两拍，只保当前集会把正在被读的流逐掉。
const KEEP_RECENT: usize = 3;

/// 预取标记的上限。标记豁免字节预算且各带一条预取头部，
/// 无界会净占上百 MB 内存；16 个足够覆盖连播/换剧两路满档的常用量。
const MAX_PREFETCH_MARKS: usize = 16;

/// 在线流缓存的总字节预算，计入的是整集明文长度（保守口径）。
/// 超预算时从最旧的非当前条目逐出；预取条目有独立的标记和填充上限。
const RETAIN_BUDGET_BYTES: u64 = 48 * 1024 * 1024;

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

    /// Capture a ready entry and its size together. A transient lease does not
    /// keep an evicted payload resident or allow a same-key replacement.
    pub fn open_ready_stream(&self, vid: &str, definition: u32) -> Option<(StreamGeneration, u64)> {
        let entry = self.get(vid, definition)?;
        let len = crate::read::entry_len(&entry)?;
        let generation = StreamGeneration {
            key: StreamKey {
                vid: vid.to_owned(),
                definition,
            },
            entry: Arc::downgrade(&entry),
        };
        Some((generation, len))
    }

    pub fn is_current_generation(&self, generation: &StreamGeneration) -> bool {
        self.entry_for_generation(generation).is_some()
    }

    fn entry_for_generation(&self, generation: &StreamGeneration) -> Option<Arc<StreamEntry>> {
        self.entries
            .lock()
            .get(&generation.key)
            .filter(|entry| std::ptr::eq(generation.entry.as_ptr(), Arc::as_ptr(entry)))
            .cloned()
    }

    pub fn cached_bytes(&self, vid: &str, definition: u32) -> u64 {
        let Some(entry) = self.get(vid, definition) else {
            return 0;
        };
        if let Some(stream) = entry.progressive.lock().as_ref() {
            return stream.sparse.downloaded();
        }
        *entry.filled.lock()
    }

    /// 这一档的条目是否还在（取流失败会被删掉）。
    pub fn exists(&self, vid: &str, definition: u32) -> bool {
        self.get(vid, definition).is_some()
    }

    /// 这一档是否已经可以供 `<video>` 取数，可以的话带回它提供的全部档位。
    /// 渐进模式注册了计划就算就绪：首帧等的是数据区间，不是整集。
    pub fn ready(&self, vid: &str, definition: u32) -> Option<Vec<VideoDefinition>> {
        let entry = self.get(vid, definition)?;
        if *entry.filled.lock() == 0 && entry.progressive.lock().is_none() {
            return None;
        }
        let definitions = entry.definitions.lock().clone();
        Some(definitions)
    }

    /// 注册渐进式流（取流编排建好计划后调用）。整集数据已写入或条目已被逐出时返回 `false`，
    /// 调用方据此停止填充——重建一个没人要的条目只会让它的填充白占串行队列。
    pub fn set_progressive(
        &self,
        generation: &StreamGeneration,
        stream: Arc<ProgressiveStream>,
    ) -> bool {
        let Some(entry) = self.entry_for_generation(generation) else {
            return false;
        };
        let filled = entry.filled.lock();
        let mut progressive = entry.progressive.lock();
        if *filled > 0 || progressive.is_some() {
            return false;
        }
        *progressive = Some(stream);
        true
    }

    /// 写入整段明文。已有数据时**不覆盖**并返回 `false`（这份数据作废）：
    /// 取流可能重复发生，按 `filled` 追加的话同一份数据写两遍会把 buffer 拼成两段。
    pub fn store(&self, generation: &StreamGeneration, bytes: &[u8]) -> bool {
        let Some(entry) = self.entry_for_generation(generation) else {
            return false;
        };
        let mut buffer = entry.buffer.lock();
        let mut filled = entry.filled.lock();
        let mut size = entry.size.lock();
        if *filled > 0 || entry.progressive.lock().is_some() {
            return false;
        }
        buffer.extend_from_slice(bytes);
        *filled = bytes.len() as u64;
        *size = bytes.len() as u64;
        true
    }

    /// 记下本集提供的全部档位。
    pub fn set_definitions(&self, generation: &StreamGeneration, definitions: &[VideoDefinition]) {
        if let Some(entry) = self.entry_for_generation(generation) {
            *entry.definitions.lock() = definitions.to_vec();
        }
    }

    /// Acquire exclusive fill ownership for this exact cache entry.
    pub fn begin_fetch(&self, vid: &str, definition: u32) -> Option<StreamGeneration> {
        let entry = self.get_or_create(vid, definition);
        let mut fetching = entry.fetching.lock();
        if *fetching {
            return None;
        }
        *fetching = true;
        Some(StreamGeneration {
            key: StreamKey {
                vid: vid.to_owned(),
                definition,
            },
            entry: Arc::downgrade(&entry),
        })
    }

    /// 取流结束（无论成功失败），放开门。
    pub fn end_fetch(&self, generation: &StreamGeneration) {
        if let Some(entry) = self.entry_for_generation(generation) {
            *entry.fetching.lock() = false;
        }
    }

    /// 这一档是否正在取流。
    pub fn is_fetching(&self, vid: &str, definition: u32) -> bool {
        self.get(vid, definition)
            .is_some_and(|e| *e.fetching.lock())
    }

    /// A failed background writer must never remove a replacement entry.
    pub fn remove_generation(&self, generation: &StreamGeneration) -> bool {
        let mut entries = self.entries.lock();
        if !entries
            .get(&generation.key)
            .is_some_and(|entry| std::ptr::eq(generation.entry.as_ptr(), Arc::as_ptr(entry)))
        {
            return false;
        }
        entries.remove(&generation.key);
        self.prefetch.lock().retain(|(vid, definition)| {
            vid != &generation.key.vid || *definition != generation.key.definition
        });
        true
    }

    /// 丢弃一档（用户显式重试或已确认是空条目时调用）。
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

    /// 逐出策略：保留当前 vid 全部档位 + 显式预取 + 最近 `KEEP_RECENT` 条其它条目，
    /// 再按总字节预算（`RETAIN_BUDGET_BYTES`）从最旧的非当前条目继续逐。
    pub fn retain_playing_set(&self, vid: &str) {
        self.retain_with(vid, KEEP_RECENT, RETAIN_BUDGET_BYTES);
    }

    fn retain_with(&self, vid: &str, keep_recent: usize, budget: u64) {
        let previous = self.previous_current.lock().clone();
        let prefetch = self.prefetch.lock().clone();
        let mut entries = self.entries.lock();
        let is_marked = |k: &StreamKey| {
            prefetch
                .iter()
                .any(|(v, d)| v == &k.vid && *d == k.definition)
        };

        let mut others: Vec<(StreamKey, std::time::Instant, u64)> = entries
            .iter()
            .filter(|(k, _)| k.vid != vid && k.vid != previous && !is_marked(k))
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

        // 字节预算优先于条数：刚被窗口留下的条目一样可能因预算出局。
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

    /// 记下当前正在看的 vid（prepare / 连播接力都走这里；逐出与填充礼让据此判断）。
    /// 写入后立刻按预算逐出：系统播放器队列接力不走 `prepare`，若只改 current
    /// 不逐出，旧集会无界堆积（长会话实测可到百条 / GB 级）。
    pub fn set_current(&self, vid: &str) {
        let mut current = self.current.lock();
        if *current != vid && !current.is_empty() {
            *self.previous_current.lock() = current.clone();
        }
        *current = vid.to_string();
        drop(current);
        self.prefetch.lock().retain(|(v, _)| v != vid);
        self.retain_playing_set(vid);
    }

    /// 这一条是否还在预取标记里（填充让位判定用）。
    pub fn is_prefetch_marked(&self, vid: &str, definition: u32) -> bool {
        self.prefetch
            .lock()
            .iter()
            .any(|(v, d)| v == vid && *d == definition)
    }

    /// 标记一条为预取（有界：超出 `MAX_PREFETCH_MARKS` 挤掉最老的）。
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

    /// 当前集的渐进流上是否还有读者在等数据——预取的有界填充据此让位：
    /// 在播那一路的供数永远优先于下一部的铺垫，条目保留、转正后续填。
    pub fn current_has_waiters(&self) -> bool {
        let current = self.current.lock().clone();
        if current.is_empty() {
            return false;
        }
        let entries = self.entries.lock();
        entries
            .iter()
            .filter(|(k, _)| k.vid == current)
            .any(|(_, e)| {
                e.progressive
                    .lock()
                    .as_ref()
                    .is_some_and(|p| p.demand.has_waiters())
            })
    }

    /// 是否还有未填的渐进数据。取流权由 begin_fetch 单独判断，避免漏掉正在退出的旧写入方。
    pub fn needs_resume(&self, vid: &str, definition: u32) -> bool {
        let Some(entry) = self.get(vid, definition) else {
            return false;
        };
        let Some(prog) = entry.progressive.lock().clone() else {
            return false;
        };
        prog.sparse.downloaded() < prog.sparse.len()
    }

    /// 取续填所需的数据面（稀疏缓冲 + 渐进计划）。仅 [`Self::needs_resume`]
    /// 为真时有值；续填的编排（取流权/事件上报）在 play_service 侧。
    pub fn resume_parts(
        &self,
        generation: &StreamGeneration,
    ) -> Option<(Arc<SparseBuffer>, Arc<ProgressiveStream>)> {
        let entry = self.entry_for_generation(generation)?;
        let prog = entry.progressive.lock().clone()?;
        Some((prog.sparse.clone(), prog))
    }

    /// Cache entries and their resident payload/plan byte cost, matching eviction accounting.
    pub fn status(&self) -> (usize, u64) {
        let guard = self.entries.lock();
        let bytes: u64 = guard.values().map(|entry| entry_weight(entry)).sum();
        (guard.len(), bytes)
    }

    /// 清空。
    pub fn clear(&self) {
        self.entries.lock().clear();
        self.prefetch.lock().clear();
        self.previous_current.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_generation_rejects_eviction_and_same_key_replacement() {
        let cache = StreamCache::default();
        let vid = "episode";
        let definition = 720;
        assert!(cache.open_ready_stream(vid, definition).is_none());

        let original_writer = cache.begin_fetch(vid, definition).unwrap();
        assert!(cache.open_ready_stream(vid, definition).is_none());
        assert!(cache.store(&original_writer, &[1; 8]));
        let (original, original_len) = cache.open_ready_stream(vid, definition).unwrap();
        assert_eq!(original_len, 8);
        assert!(cache.is_current_generation(&original));

        cache.remove(vid, definition);
        assert!(!cache.is_current_generation(&original));
        let new_writer = cache.begin_fetch(vid, definition).unwrap();
        cache.end_fetch(&original_writer);
        assert!(cache.is_fetching(vid, definition));
        assert!(!cache.store(&original_writer, &[3; 24]));
        assert!(!cache.remove_generation(&original_writer));
        assert!(cache.store(&new_writer, &[2; 16]));
        let (replacement, new_len) = cache.open_ready_stream(vid, definition).unwrap();
        assert_eq!(new_len, 16);
        assert!(cache.is_current_generation(&replacement));
        assert!(!cache.is_current_generation(&original));
        assert!(original.entry.upgrade().is_none());
    }

    #[test]
    fn incomplete_stream_still_needs_resume_while_another_writer_exits() {
        let cache = StreamCache::default();
        let vid = "outgoing";
        let definition = 1080;
        let sparse = SparseBuffer::new(16);
        sparse.write(0, &[0; 8]);
        let progress = Arc::new(ProgressiveStream {
            sparse: sparse.clone(),
            plan: None,
            plain_len: sparse.len(),
            demand: Default::default(),
            cdn_url: String::new(),
            media_cache_key: None,
            proxy: Default::default(),
        });
        let writer = cache.begin_fetch(vid, definition).unwrap();
        assert!(cache.set_progressive(&writer, progress.clone()));
        assert!(!cache.set_progressive(&writer, progress));
        assert!(!cache.store(&writer, &[2; 16]));

        assert!(cache.needs_resume(vid, definition));
        assert!(cache.begin_fetch(vid, definition).is_none());

        cache.end_fetch(&writer);
        assert!(cache.begin_fetch(vid, definition).is_some());
        sparse.write(8, &[0; 8]);
        assert!(!cache.needs_resume(vid, definition));
    }
}
