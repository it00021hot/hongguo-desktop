//! chunk → 样本 的换算。
//!
//! `stsz` 只给样本**大小**、`stco`/`co64` 只给 chunk **起点**，
//! 中间缺的一环是 `stsc`：「每个 chunk 装几个样本」。
//!
//! ⚠️ 缺了这一步，`samples[i].0` 会全停在 0，解密就会去读文件头——
//!    **不报错**，只是产物全是噪声。所以这里是独立一个文件，便于单独看。

use super::sample_table::TrackInfo;

/// 从 `stsc` 读「每 chunk 多少样本」。
///
/// 结构：version+flags(4) | entry_count(4) |
/// entries: first_chunk(4) samples_per_chunk(4) sample_description_index(4)
///
/// 只保留前两个字段——`sample_description_index` 在短剧这类单描述轨道上恒为 1。
pub(super) fn read_stsc(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    if size < 8 {
        return;
    }
    // 畸形 count 防御（口径同 timing::read_entry_count）：卡到 box 容量内
    let count = (u32::from_be_bytes([
        data[start + 4],
        data[start + 5],
        data[start + 6],
        data[start + 7],
    ]) as usize)
        .min(size.saturating_sub(8) / 12)
        .min(data.len());

    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let at = start + 8 + i * 12;
        if at + 12 > start + size {
            break;
        }
        let rd = |o: usize| {
            u32::from_be_bytes([
                data[at + o],
                data[at + o + 1],
                data[at + o + 2],
                data[at + o + 3],
            ])
        };
        entries.push((rd(0), rd(4)));
    }
    // 规范要求 first_chunk 严格升序；乱序会让下面的二分查找失效
    entries.sort_by_key(|(first, _)| *first);
    info.stsc = entries;
}

/// 用 `stsc` + `stco` 把 chunk 偏移展开成每个样本的绝对偏移。
///
/// `stsz` 只给样本大小、`stco` 只给 chunk 起点，两者的换算必须靠 `stsc`
/// 说明「每个 chunk 装几个样本」。缺了这一步，`samples[i].0` 会全停在 0，
/// 解密就会去读文件头——**不报错，但产物是乱码**。
pub(super) fn resolve_sample_offsets(info: &mut TrackInfo) {
    if info.chunk_offsets.is_empty() || info.stsc.is_empty() || info.samples.is_empty() {
        return;
    }

    let sizes: Vec<u64> = info.samples.iter().map(|(_, s)| *s).collect();
    let mut resolved: Vec<(u64, u64)> = Vec::with_capacity(sizes.len());
    let mut next = 0usize;

    for (chunk_index, &chunk_start) in info.chunk_offsets.iter().enumerate() {
        // first_chunk 是 1 基
        let chunk_no = chunk_index as u32 + 1;
        let per_chunk = info
            .stsc
            .iter()
            .rev()
            .find(|(first, _)| *first <= chunk_no)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        if per_chunk == 0 {
            continue;
        }

        let mut at = chunk_start;
        for _ in 0..per_chunk {
            let Some(&size) = sizes.get(next) else { break };
            resolved.push((at, size));
            at += size;
            next += 1;
        }
        if next >= sizes.len() {
            break;
        }
    }

    // 样本表被截断时保留未覆盖的部分，避免调用方误判「已全部解析」
    if resolved.len() < sizes.len() {
        for &size in &sizes[resolved.len()..] {
            resolved.push((0, size));
        }
    }
    info.samples = resolved;
}
