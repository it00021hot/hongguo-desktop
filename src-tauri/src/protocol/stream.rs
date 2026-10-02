//! 在线播放的内存流（`hongguo-stream://`）。
//!
//! 播放器通过 Range 反复请求同一份数据面：已就绪的区间立即返回，
//! 还没填充完的区间**挂起等待**而不是报错。写入方是
//! [`crate::service::play_service::online`]——它取流、解密后把明文推进缓存。
//!
//! 开放式 Range 只回已填充窗口，让 `<video>` 自己按 Range 续取。

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::range::{parse_range, partial_headers, ProtocolResponse, RangeSpec};

/// 开放式 Range 的单次供给窗口。
const SERVE_WINDOW: u64 = 512 * 1024;

/// 等待数据就绪的最长时间（秒）。超时返回 502，避免请求永久挂起。
const WAIT_TIMEOUT_SECS: u64 = 30;

/// 供给在线流的一次请求。
///
/// 请求落到还没填充的区间时**等待**数据就绪（`Notify`），而不是立即报错——
/// 这就是「边下边看」的数据面。开放式 Range 只回已填充窗口。
pub fn serve(vid: &str, range_header: Option<&str>) -> Result<ProtocolResponse, String> {
    // 一个 12MB 的一集会打十几条，所以放 debug：排查「WebView 到底有没有
    // 请求这条协议」时用 RUST_LOG=debug 打开，日常不刷屏。
    log::debug!("[Stream] 请求 vid={vid} range={range_header:?}");
    let entry = crate::service::play_service::online::cache().get(vid);
    if entry.is_none() {
        return Err(format!("没有正在准备的在线流: {vid}"));
    }
    let entry = entry.expect("刚判定过存在");

    // 总大小要等 moov 到齐（计划就绪）才知道
    let size = wait_until(Duration::from_secs(WAIT_TIMEOUT_SECS), || {
        *entry.size.lock() > 0
    })
    .then(|| *entry.size.lock())
    .ok_or_else(|| "等待流就绪超时".to_string())?;

    // 等到请求区间已填充
    let range = parse_range(range_header, size);
    wait_until(Duration::from_secs(WAIT_TIMEOUT_SECS), || {
        try_extract(&entry, range, size).is_some()
    })
    .then(|| try_extract(&entry, range, size).expect("刚等到的数据仍然在"))
    .ok_or_else(|| "等待数据就绪超时".to_string())
}

/// 轮询等待条件成立。协议 handler 跑在专用工作线程上，轮询足够且简单可靠
/// （比在协议线程里架一套 tokio runtime 更不容易出死锁）。
fn wait_until(timeout: std::time::Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cond() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// 尝试按 Range 取出数据，取不到返回 `None`（调用方继续等）。
fn try_extract(entry: &Arc<StreamEntry>, range: RangeSpec, size: u64) -> Option<ProtocolResponse> {
    let filled = *entry.filled.lock();
    let buffer = entry.buffer.lock();

    match range {
        RangeSpec::Full => {
            if filled < size {
                return None; // 还没下完，等
            }
            Some((
                200,
                vec![
                    ("Content-Type".into(), "video/mp4".into()),
                    ("Accept-Ranges".into(), "bytes".into()),
                    ("Content-Length".into(), size.to_string()),
                ],
                buffer[..size as usize].to_vec(),
            ))
        }
        RangeSpec::Closed { start, end } => {
            let need = end + 1;
            if filled < need {
                return None;
            }
            Some((
                206,
                partial_headers(start, end, size),
                buffer[start as usize..need as usize].to_vec(),
            ))
        }
        RangeSpec::Open { start } => {
            // 开放式：只要这一位有数据就回一个已填充窗口
            if filled <= start {
                return None;
            }
            let end = std::cmp::min(start + SERVE_WINDOW - 1, filled - 1);
            Some((
                206,
                partial_headers(start, end, size),
                buffer[start as usize..=end as usize].to_vec(),
            ))
        }
        RangeSpec::Unsatisfiable => Some((
            416,
            vec![
                ("Content-Range".into(), format!("bytes */{size}")),
                ("Content-Type".into(), "text/plain".into()),
            ],
            Vec::new(),
        )),
    }
}

/// 一个正在填充的流。
#[derive(Default)]
pub struct StreamEntry {
    /// 已解密的明文缓冲
    pub buffer: Mutex<Vec<u8>>,
    /// 已填充到的位置（字节数）
    pub filled: Mutex<u64>,
    /// 总大小
    pub size: Mutex<u64>,
}

/// 全部在线流的注册表。
#[derive(Default)]
pub struct StreamCache {
    entries: Mutex<std::collections::HashMap<String, Arc<StreamEntry>>>,
}

impl StreamCache {
    /// 取或创建一个 entry。
    pub fn entry(&self, vid: &str) -> Arc<StreamEntry> {
        let mut guard = self.entries.lock();
        guard
            .entry(vid.to_string())
            .or_insert_with(|| Arc::new(StreamEntry::default()))
            .clone()
    }

    /// 查一个 entry。
    pub fn get(&self, vid: &str) -> Option<Arc<StreamEntry>> {
        self.entries.lock().get(vid).cloned()
    }

    /// 写入一段明文。写入后 entry 立刻可被 `serve` 供给。
    pub fn append(&self, vid: &str, bytes: &[u8]) {
        let entry = self.entry(vid);
        let mut buf = entry.buffer.lock();
        let mut filled = entry.filled.lock();
        let at = *filled as usize;
        if buf.len() < at + bytes.len() {
            buf.resize(at + bytes.len(), 0);
        }
        buf[at..at + bytes.len()].copy_from_slice(bytes);
        *filled += bytes.len() as u64;
    }

    /// 设置总大小。`serve` 等它就绪后才开始供给。
    pub fn set_size(&self, vid: &str, size: u64) {
        *self.entry(vid).size.lock() = size;
    }

    /// 丢弃一个流（失败或被取消时调用）。
    pub fn remove(&self, vid: &str) {
        self.entries.lock().remove(vid);
    }

    /// 缓存状态：(流数, 已缓存字节)。
    pub fn status(&self) -> (usize, u64) {
        let guard = self.entries.lock();
        let bytes: u64 = guard.values().map(|e| *e.filled.lock()).sum();
        (guard.len(), bytes)
    }

    /// 清空。
    pub fn clear(&self) {
        self.entries.lock().clear();
    }

    /// 在线流 URL。
    pub fn stream_url(&self, vid: &str) -> String {
        super::stream_url(vid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_back() {
        let c = StreamCache::default();
        c.append("v1", b"hello ");
        c.append("v1", b"world");
        let e = c.entry("v1");
        assert_eq!(&*e.buffer.lock(), b"hello world");
        assert_eq!(*e.filled.lock(), 11);
    }

    #[test]
    fn set_size_is_visible_to_serve() {
        let c = StreamCache::default();
        c.set_size("v2", 1000);
        assert_eq!(*c.entry("v2").size.lock(), 1000);
    }

    #[test]
    fn status_counts_entries_and_bytes() {
        let c = StreamCache::default();
        c.append("a", &[0u8; 100]);
        c.append("b", &[0u8; 50]);
        let (count, bytes) = c.status();
        assert_eq!(count, 2);
        assert_eq!(bytes, 150);
    }

    #[test]
    fn clear_removes_all() {
        let c = StreamCache::default();
        c.append("a", &[0u8; 10]);
        c.clear();
        assert_eq!(c.status().0, 0);
    }

    #[test]
    fn remove_drops_single_stream() {
        let c = StreamCache::default();
        c.append("a", &[0u8; 10]);
        c.append("b", &[0u8; 10]);
        c.remove("a");
        assert!(c.get("a").is_none());
        assert!(c.get("b").is_some());
    }

    #[test]
    fn stream_url_format() {
        let c = StreamCache::default();
        // 必须是 http://{scheme}.localhost 形式，否则 WebView2 不会请求
        assert_eq!(
            c.stream_url("v1"),
            "http://hongguo-stream.localhost/v1"
        );
    }

    #[test]
    fn range_parsing_on_stream() {
        let spec = parse_range(Some("bytes=0-99"), 1000);
        assert_eq!(spec, RangeSpec::Closed { start: 0, end: 99 });
        let headers = partial_headers(0, 99, 1000);
        assert_eq!(headers[2].1, "bytes 0-99/1000");
    }

    // ---- 播放数据面：<video> 实际会发的几类请求 ----

    /// 造一个「已填好 size + 全部字节」的流。
    fn ready_stream(vid: &str, len: usize) {
        let c = crate::service::play_service::online::cache();
        c.remove(vid);
        c.set_size(vid, len as u64);
        c.append(vid, &(0..len).map(|i| (i % 251) as u8).collect::<Vec<u8>>());
    }

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn serves_full_body_after_fill() {
        ready_stream("v-full", 4096);
        let (status, headers, body) = serve("v-full", None).expect("整文件请求应成功");
        assert_eq!(status, 200);
        assert_eq!(body.len(), 4096);
        assert_eq!(header(&headers, "Content-Type"), Some("video/mp4"));
        assert_eq!(header(&headers, "Accept-Ranges"), Some("bytes"));
        assert_eq!(header(&headers, "Content-Length"), Some("4096"));
    }

    #[test]
    fn serves_closed_range_for_seeking() {
        ready_stream("v-closed", 4096);
        let (status, headers, body) = serve("v-closed", Some("bytes=100-199")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body.len(), 100);
        assert_eq!(header(&headers, "Content-Range"), Some("bytes 100-199/4096"));
        assert_eq!(header(&headers, "Content-Length"), Some("100"));
    }

    #[test]
    fn open_range_returns_filled_window() {
        // 开放式 Range 是 <video> 起播的第一枪，必须给 206 + Content-Range
        ready_stream("v-open", 4096);
        let (status, headers, body) = serve("v-open", Some("bytes=0-")).unwrap();
        assert_eq!(status, 206);
        assert!(!body.is_empty());
        let cr = header(&headers, "Content-Range").expect("开放式也要 Content-Range");
        assert!(cr.starts_with("bytes 0-"), "实际: {cr}");
        assert!(cr.ends_with("/4096"), "实际: {cr}");
    }

    #[test]
    fn unknown_vid_is_an_error_not_a_hang() {
        let err = serve("never-prepared", Some("bytes=0-")).expect_err("未准备的流应报错");
        assert!(err.contains("没有正在准备的在线流"), "实际: {err}");
    }

    #[test]
    fn out_of_bounds_range_is_416() {
        ready_stream("v-416", 100);
        let (status, headers, body) = serve("v-416", Some("bytes=500-600")).unwrap();
        assert_eq!(status, 416);
        assert_eq!(header(&headers, "Content-Range"), Some("bytes */100"));
        assert!(body.is_empty());
    }
}
