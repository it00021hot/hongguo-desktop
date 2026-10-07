//! 流式解密：不整集等待的明文视图。
//!
//! 在线播放的原路径是「整集取回 → 整集解密 → 原子交给协议」，首帧要等
//! 全部字节。本模块把它改成**按需**：只凭 moov 就能回答「明文第 X 字节
//! 来自密文哪里、要不要异或 keystream」，协议层按 Range 取一段、解一段。
//!
//! ## 为什么可行（三条事实，缺一不可）
//!
//! 1. **CTR 解密不改样本长度**，重建 moov 只动 moov 自己——moov 之前的
//!    明文偏移与密文偏移**一一对应**；moov 之后的内容按
//!    `新 moov 长 − 旧 moov 长` 整体平移。
//! 2. **CTR 是计数器模式**，样本内任意子区间都能独立解（见
//!    [`crate::domain::crypto::cenc::decrypt_range`]），不需要样本前缀。
//! 3. **异或可交换**：多轨样本重叠时，整文件解密是多次原地异或，
//!    与「逐字节异或所有覆盖样本的 keystream」等价——所以这里不必关心
//!    轨道顺序与重叠方式，逐字节与 [`super::decrypt_buffer`] 一致
//!    （由测试锁住）。
//!
//! 布局保持原位替换（新 moov 顶旧 moov 的位置，见 [`super::deprotect`]），
//! 不合成 faststart：WebView2 自己会对尾部 moov 发 Range 请求，
//! `hongguo-local://` 播放 moov 在尾部的本地分集一直是日常主路径。

use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use super::deprotect::rebuild_moov_region;
use super::sample_table::collect_tracks;
use crate::error::{AppError, AppResult};

/// 一个待解密的样本段（跨轨合并后的原子单位）。
#[derive(Debug, Clone)]
struct SampleSeg {
    /// 密文中的绝对起点
    offset: u64,
    size: u64,
    /// 该样本的 8 字节 IV
    iv: [u8; 8],
}

/// 一条流式解密计划：moov 就位后构建一次，全程只读。
pub struct StreamingPlan {
    key: [u8; 16],
    /// 密文总长
    cipher_len: u64,
    /// 旧 moov box 在密文中的起点（含 box 头）
    moov_start: u64,
    /// 重建后的 moov（stco 已按原位替换平移）
    new_moov: Vec<u8>,
    /// 明文总长
    plain_len: u64,
    /// 按偏移升序的样本段（跨轨道合并）
    segments: Vec<SampleSeg>,
}

impl StreamingPlan {
    /// 从 moov 区域构建计划。
    ///
    /// `moov_region` 是从旧 moov box 头开始的密文切片（尾部预取或头部
    /// 缓冲里截出来的），`moov_abs_start` 是它在完整密文中的绝对偏移。
    /// 区间内的 stco 值是绝对文件偏移，所以切片解析与整文件解析同构。
    pub fn build(
        moov_region: &[u8],
        moov_abs_start: u64,
        cipher_len: u64,
        key: &[u8; 16],
    ) -> AppResult<Self> {
        let tracks = collect_tracks(moov_region)
            .map_err(|e| AppError::Decrypt(format!("流式计划解析样本表失败: {e}")))?;
        if tracks.is_empty() {
            return Err(AppError::Decrypt("没有可用轨道".into()));
        }

        // 与 decrypt_mp4_buffer 同一口径：有 IV 没样本的轨道是坏数据
        for t in &tracks {
            if !t.sample_ivs.is_empty() && t.samples.is_empty() {
                return Err(AppError::Decrypt(format!("轨道 {} 没有样本", t.track_id)));
            }
        }

        let mut segments: Vec<SampleSeg> = Vec::new();
        for t in &tracks {
            for (i, &(offset, size)) in t.samples.iter().enumerate() {
                let Some(&iv) = t.sample_ivs.get(i) else {
                    continue; // 没有逐样本 IV 的样本不解密（与整文件路径一致）
                };
                if offset + size > cipher_len {
                    return Err(AppError::Decrypt(format!(
                        "轨道 {} 样本 {i} 越界（{offset}+{size} > {cipher_len}）",
                        t.track_id
                    )));
                }
                segments.push(SampleSeg { offset, size, iv });
            }
        }
        segments.sort_by_key(|s| s.offset);

        let rebuilt = rebuild_moov_region(moov_region, moov_abs_start as usize)
            .map_err(|e| AppError::Decrypt(format!("流式计划重建 moov 失败: {e}")))?;
        let (new_moov, old_moov_end) = match rebuilt {
            Some(r) => (r.bytes, r.end as u64),
            None => {
                // 没有加密标记：调用方对明文流不建计划，走到这里按
                // 「明文原样保留」兜底（与 deprotect 的处理一致）
                (
                    moov_region.to_vec(),
                    moov_abs_start + moov_region.len() as u64,
                )
            }
        };

        let plain_len = moov_abs_start + new_moov.len() as u64 + (cipher_len - old_moov_end);

        Ok(Self {
            key: *key,
            cipher_len,
            moov_start: moov_abs_start,
            plain_len,
            new_moov,
            segments,
        })
    }

    /// 明文总长。
    pub fn plain_len(&self) -> u64 {
        self.plain_len
    }

    /// 明文 moov 与旧 moov 的长度差：明文偏移 → 密文偏移的平移量。
    fn tail_delta(&self) -> i64 {
        // 明文里 moov 之后的第 X 字节 ↔ 密文里 X − delta 处。
        // 旧 moov 长 = new_len + cipher_len − plain_len（由 plain_len 的构成式反解），
        // 用 i64：head-moov 布局下 plain_len < cipher_len，u64 直接减会下溢
        let new_len = self.new_moov.len() as i64;
        let old_len = new_len + self.cipher_len as i64 - self.plain_len as i64;
        new_len - old_len
    }

    /// 明文区间 `[start, end)` 依赖的密文区间（用于判断数据是否就位）。
    ///
    /// 返回升序区间；明文里的 moov 段不依赖任何密文。
    pub fn cipher_ranges_needed(&self, start: u64, end: u64) -> Vec<(u64, u64)> {
        let mut out: Vec<(u64, u64)> = Vec::new();
        let moov_plain_end = self.moov_start + self.new_moov.len() as u64;

        if start < self.moov_start {
            out.push((start, end.min(self.moov_start)));
        }
        if end > moov_plain_end {
            let delta = self.tail_delta();
            let s = start.max(moov_plain_end);
            let cs = (s as i64 - delta) as u64;
            let ce = (end as i64 - delta) as u64;
            match out.last_mut() {
                Some(last) if last.1 == cs => last.1 = ce,
                _ => out.push((cs, ce)),
            }
        }
        out
    }

    /// 渲染明文区间 `[start, end)` 进 `out`（长度必须等于 `end − start`）。
    ///
    /// **调用方必须先保证** [`Self::cipher_ranges_needed`] 给出的密文区间
    /// 已在 `sparse` 里就位；这里不做等待，只做映射与解密。
    pub fn render(&self, start: u64, end: u64, sparse: &SparseBuffer, out: &mut [u8]) {
        assert_eq!(out.len(), (end - start) as usize, "out 长度必须等于区间长");
        let moov_plain_end = self.moov_start + self.new_moov.len() as u64;

        // 区间 1：moov 之前（明文偏移 == 密文偏移）
        if start < self.moov_start {
            let e = end.min(self.moov_start);
            self.render_cipher(start, e, sparse, out, 0);
        }
        // 区间 2：moov 本体
        if start < moov_plain_end && end > self.moov_start {
            let s = start.max(self.moov_start) - self.moov_start;
            let e = (end - self.moov_start).min(self.new_moov.len() as u64);
            let out_from = (start.max(self.moov_start) - start) as usize;
            out[out_from..out_from + (e - s) as usize]
                .copy_from_slice(&self.new_moov[s as usize..e as usize]);
        }
        // 区间 3：moov 之后（平移映射回密文）
        if end > moov_plain_end {
            let delta = self.tail_delta();
            let ps = start.max(moov_plain_end);
            let cs = (ps as i64 - delta) as u64;
            let out_from = (ps - start) as usize;
            self.render_cipher(cs, cs + (end - ps), sparse, out, out_from);
        }
    }

    /// 把密文区间 `[cs, ce)` 变换后写进 `out[out_base..]`。
    ///
    /// 先原样拷贝，再对覆盖到的样本段逐个异或 keystream——重叠段天然按
    /// 异或组合，与整文件解密的多次原地异或等价。
    fn render_cipher(
        &self,
        cs: u64,
        ce: u64,
        sparse: &SparseBuffer,
        out: &mut [u8],
        out_base: usize,
    ) {
        if cs >= ce {
            return;
        }
        sparse.read_into(cs, &mut out[out_base..out_base + (ce - cs) as usize]);

        // 二分跳过「完全在 cs 之前结束」的段；前面可能覆盖 cs 的段都在结果里
        let begin = self.segments.partition_point(|s| s.offset + s.size <= cs);
        for seg in &self.segments[begin..] {
            if seg.offset >= ce {
                break;
            }
            let overlap_s = seg.offset.max(cs);
            let overlap_e = (seg.offset + seg.size).min(ce);
            if overlap_s >= overlap_e {
                continue;
            }
            let out_at = out_base + (overlap_s - cs) as usize;
            let skip = (overlap_s - seg.offset) as usize;
            let len = (overlap_e - overlap_s) as usize;
            crate::domain::crypto::cenc::decrypt_range(
                &self.key,
                &seg.iv,
                skip,
                &mut out[out_at..out_at + len],
            );
        }
    }
}

/// 在缓冲里定位 moov box。
///
/// `buf` 是已取回的一段密文，`base` 是它的绝对起点。moov 的四字符码在
/// 密文里也可能作为普通样本数据出现，所以要验证声明的 box 长度能落在
/// 文件内；多个候选时优先「恰好到文件末尾」的那个（moov 通常是最后
/// 一个 box）。
///
/// 返回 `(绝对起点, 含头总长)`。
pub fn locate_moov(buf: &[u8], base: u64, file_len: u64) -> Option<(u64, u64)> {
    let mut first_valid: Option<(u64, u64)> = None;
    let mut exact_end: Option<(u64, u64)> = None;
    let mut pos = 0usize;
    while let Some(rel) = find_fourcc(buf, b"moov", pos) {
        if rel >= 4 {
            let size = u32::from_be_bytes([
                buf[rel - 4],
                buf[rel - 3],
                buf[rel - 2],
                buf[rel - 1],
            ]) as u64;
            // size 为 0（到末尾）或 1（largesize）的 moov 罕见到不必支持
            if size >= 8 {
                let abs = base + (rel - 4) as u64;
                if abs + size <= file_len {
                    let cand = (abs, size);
                    if abs + size == file_len {
                        exact_end = Some(cand);
                    }
                    first_valid.get_or_insert(cand);
                }
            }
        }
        pos = rel + 4;
    }
    exact_end.or(first_valid)
}

fn find_fourcc(buf: &[u8], needle: &[u8; 4], from: usize) -> Option<usize> {
    if buf.len() < needle.len() {
        return None;
    }
    (from..=buf.len() - needle.len()).find(|&i| &buf[i..i + needle.len()] == needle)
}

/// 解析一段**从盒边界开始**的顶层盒表，返回最后一个「头部完整」的盒的
/// 结束偏移（绝对坐标 = `base + 头部推算出的盒尾`）。
///
/// 盒本体允许伸出缓冲外——mdat 的 size 写在 8 字节头里，256KB 首探就
/// 足够算出「mdat 之后 = 尾部 moov 的落点」，这正是 hgplayer 盒游走
/// `it(f, f+64*1024)` 的跳转依据。size==1 按 largesize（16 字节头）解析；
/// size==0（到 EOF）视同解析终止（hgplayer 的 Ht 同样不认）。
pub fn top_boxes_end(buf: &[u8], base: u64) -> Option<u64> {
    let mut pos = 0usize;
    let mut end = base;
    while pos + 8 <= buf.len() {
        let mut size =
            u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as u64;
        let header = if size == 1 {
            if pos + 16 > buf.len() {
                break;
            }
            let mut wide = [0u8; 8];
            wide.copy_from_slice(&buf[pos + 8..pos + 16]);
            size = u64::from_be_bytes(wide);
            16u64
        } else {
            8u64
        };
        // size==0（到 EOF）或损坏头：后面没法定位，就此打住
        if size < header {
            break;
        }
        end = base.saturating_add(pos as u64).saturating_add(size);
        pos += size as usize;
        // 盒体伸出缓冲（典型：256KB 首探里的巨型 mdat）——头部已算完
        if pos > buf.len() {
            break;
        }
    }
    (end > base).then_some(end)
}

// ---------------------------------------------------------------- 稀疏缓冲

/// 稀疏密文缓冲：已下载区间 + 就绪通知。
///
/// 整集常驻内存与现行在线播放同一量级（今天还要同时留密文与明文两份
/// 过渡副本，这里只有密文一份 + 应答时的小缓冲）。
pub struct SparseBuffer {
    len: u64,
    data: Mutex<Vec<u8>>,
    /// 已就绪区间，升序且互不重叠
    covered: Mutex<Vec<(u64, u64)>>,
    cv: Condvar,
}

impl SparseBuffer {
    pub fn new(len: u64) -> Arc<Self> {
        Arc::new(Self {
            len,
            data: Mutex::new(vec![0u8; len as usize]),
            covered: Mutex::new(Vec::new()),
            cv: Condvar::new(),
        })
    }

    /// 总长。
    pub fn len(&self) -> u64 {
        self.len
    }

    /// 写入一段密文并唤醒等待者。
    pub fn write(&self, start: u64, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        {
            let mut data = self.data.lock();
            let s = start as usize;
            data[s..s + bytes.len()].copy_from_slice(bytes);
        }
        let mut covered = self.covered.lock();
        merge_run(&mut covered, start, start + bytes.len() as u64);
        self.cv.notify_all();
    }

    /// `[start, end)` 是否已就绪。
    pub fn covers(&self, start: u64, end: u64) -> bool {
        let covered = self.covered.lock();
        covered.iter().any(|(a, b)| *a <= start && end <= *b)
    }

    /// 已就绪的字节数（进度用）。
    pub fn downloaded(&self) -> u64 {
        self.covered.lock().iter().map(|(a, b)| b - a).sum()
    }

    /// 拷出一段密文。**不做就绪检查**——调用方先 `covers`。
    pub fn read_into(&self, start: u64, out: &mut [u8]) {
        let data = self.data.lock();
        let s = start as usize;
        out.copy_from_slice(&data[s..s + out.len()]);
    }

    /// 从 `from` 起找第一个未覆盖区间；全满返回 `None`。
    pub fn next_gap(&self, from: u64) -> Option<(u64, u64)> {
        let covered = self.covered.lock();
        let mut cursor = from.min(self.len);
        for (a, b) in covered.iter() {
            if *b <= cursor {
                continue;
            }
            if *a > cursor {
                return Some((cursor, (*a).min(self.len)));
            }
            cursor = *b;
        }
        (cursor < self.len).then_some((cursor, self.len))
    }

    /// 等待全部区间就绪。超时返回 `false`。
    pub fn wait_cover(&self, ranges: &[(u64, u64)], timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut covered = self.covered.lock();
        loop {
            if ranges
                .iter()
                .all(|(a, b)| covered.iter().any(|(x, y)| *x <= *a && *b <= *y))
            {
                return true;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            self.cv.wait_for(&mut covered, left);
        }
    }
}

/// 把 `[start, end)` 合并进升序区间列表。
fn merge_run(runs: &mut Vec<(u64, u64)>, start: u64, end: u64) {
    let mut merged = (start, end);
    let mut out: Vec<(u64, u64)> = Vec::with_capacity(runs.len() + 1);
    for (a, b) in runs.iter() {
        if *b < merged.0 || *a > merged.1 {
            out.push((*a, *b));
        } else {
            merged.0 = merged.0.min(*a);
            merged.1 = merged.1.max(*b);
        }
    }
    out.push(merged);
    out.sort();
    *runs = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个顶层盒字节段：`[size:4][type:4]` + size 大小的本体。
    fn box_bytes(size: u64, kind: &[u8; 4], body_fill: u8) -> Vec<u8> {
        let mut v = (size as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.resize(v.len() + size.saturating_sub(8) as usize, body_fill);
        v
    }

    #[test]
    fn top_boxes_end_computes_mdat_end_from_header_alone() {
        // 尾-moov 布局：ftyp(32) + 巨型 mdat(1_000_000) —— 缓冲里只有 mdat 头，
        // 盒尾仍应从头部 size 算出（256KB 首探定位尾部 moov 的关键）
        let mut buf = box_bytes(32, b"ftyp", 0);
        buf.extend(box_bytes(1_000_000, b"mdat", 0)); // 本体被截断
        assert_eq!(top_boxes_end(&buf, 0), Some(32 + 1_000_000));
    }

    #[test]
    fn top_boxes_end_walks_complete_boxes() {
        let mut buf = box_bytes(20, b"ftyp", 1);
        buf.extend(box_bytes(40, b"free", 2));
        buf.extend(box_bytes(60, b"mdat", 3));
        assert_eq!(top_boxes_end(&buf, 0), Some(120));
        // 非零 base（游走的第二段）：绝对坐标
        assert_eq!(top_boxes_end(&buf, 5000), Some(5120));
    }

    #[test]
    fn top_boxes_end_stops_on_zero_size_box() {
        // size==0（到 EOF）不支持，视同解析终止：只算到前一个盒的尾
        let mut buf = box_bytes(16, b"ftyp", 0);
        let mut zero = 0u32.to_be_bytes().to_vec();
        zero.extend_from_slice(b"mdat");
        zero.extend_from_slice(&[0u8; 8]);
        buf.extend(zero);
        assert_eq!(top_boxes_end(&buf, 0), Some(16));
    }

    #[test]
    fn top_boxes_end_parses_largesize_header() {
        // size==1 → largesize(u64)：16 字节头，本体允许伸出缓冲
        let mut v = 1u32.to_be_bytes().to_vec();
        v.extend_from_slice(b"mdat");
        v.extend_from_slice(&100_000u64.to_be_bytes());
        v.extend_from_slice(&[0u8; 32]);
        assert_eq!(top_boxes_end(&v, 0), Some(100_000));
    }

    #[test]
    fn top_boxes_end_rejects_garbage() {
        assert_eq!(top_boxes_end(&[], 0), None);
        assert_eq!(top_boxes_end(&[1, 2, 3], 0), None);
        // 头部 size 小于 8：损坏，解析终止
        assert_eq!(top_boxes_end(&[0, 0, 0, 4, b'm', 0, 0, 0], 0), None);
    }

    #[test]
    fn sparse_buffer_tracks_runs() {
        let b = SparseBuffer::new(1000);
        b.write(100, &[7u8; 50]);
        assert!(b.covers(100, 150));
        assert!(!b.covers(99, 150));
        assert!(!b.covers(100, 151));
        assert_eq!(b.downloaded(), 50);

        b.write(150, &[7u8; 50]); // 相邻合并
        assert!(b.covers(100, 200));
        assert_eq!(b.downloaded(), 100);

        b.write(0, &[7u8; 30]); // 与 100 不相邻
        assert!(!b.covers(0, 200));
        b.write(30, &[7u8; 70]); // 填上中间
        assert!(b.covers(0, 200));
    }

    #[test]
    fn next_gap_walks_holes() {
        let b = SparseBuffer::new(100);
        b.write(10, &[0u8; 20]); // [10,30)
        b.write(50, &[0u8; 10]); // [50,60)
        assert_eq!(b.next_gap(0), Some((0, 10)));
        assert_eq!(b.next_gap(10), Some((30, 50)));
        assert_eq!(b.next_gap(55), Some((60, 100)));
        b.write(0, &[0u8; 100]);
        assert_eq!(b.next_gap(0), None, "全满后没有洞");
    }

    #[test]
    fn wait_cover_times_out_when_never_ready() {
        let b = SparseBuffer::new(100);
        assert!(!b.wait_cover(&[(0, 100)], Duration::ZERO));
    }

    #[test]
    fn wait_cover_returns_when_writer_fills() {
        let b = SparseBuffer::new(10);
        let bg = b.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            bg.write(0, &[1u8; 10]);
        });
        assert!(b.wait_cover(&[(0, 10)], Duration::from_secs(5)));
    }

    #[test]
    fn locate_finds_tail_moov() {
        // [ftyp][mdat][moov]：moov 应被定位到且长度精确到文件尾
        let ftyp = super::super::r#box::build_box(b"ftyp", b"isom");
        let mdat = super::super::r#box::build_box(b"mdat", &[0u8; 40]);
        let moov = super::super::r#box::build_box(b"moov", &[0u8; 60]);
        let mut file = ftyp.clone();
        file.extend_from_slice(&mdat);
        file.extend_from_slice(&moov);
        let len = file.len() as u64;
        let moov_abs = (ftyp.len() + mdat.len()) as u64;

        // 模拟只有尾部预取的那段（恰好从 moov 头开始）
        let tail = &file[moov_abs as usize..];
        assert_eq!(
            locate_moov(tail, moov_abs, len),
            Some((moov_abs, moov.len() as u64))
        );
    }

    #[test]
    fn locate_rejects_false_fourcc_inside_data() {
        // mdat 载荷里埋一个假 "moov" 四字符码，长度声明越界 → 不能认
        let mut payload = vec![0u8; 20];
        payload[4..8].copy_from_slice(b"moov");
        payload[0..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let mdat = super::super::r#box::build_box(b"mdat", &payload);
        assert!(locate_moov(&mdat, 0, mdat.len() as u64).is_none());
    }

    #[test]
    fn locate_prefers_the_exact_end_candidate() {
        // 两个合法候选：一个在中间，一个恰好到文件尾——取后者
        let inner = super::super::r#box::build_box(b"moov", &[0u8; 10]);
        let mut head = super::super::r#box::build_box(b"free", &[0u8; 4]);
        head.extend_from_slice(&inner);
        let last = super::super::r#box::build_box(b"moov", &[0u8; 12]);
        let mut file = head;
        file.extend_from_slice(&last);
        let len = file.len() as u64;
        // 最后那个 moov 的起点：free(12) + 中间 moov(8+10)
        let last_moov = (12 + 8 + 10) as u64;
        let (abs, size) = locate_moov(&file, 0, len).unwrap();
        assert_eq!((abs, size), (last_moov, (8 + 12) as u64));
        assert_eq!(abs + size, len, "应选精确到尾的候选");
    }
}
