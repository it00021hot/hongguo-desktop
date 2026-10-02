//! 样本表收集。
//!
//! 从 `stbl` 里读出每个样本的偏移与大小，以及加密所需的 `saiz` / `saio` /
//! `senc` 辅助信息。这是 CENC 解密的入口数据。

use super::cenc_info;
use super::chunk_map;
use super::r#box::{find_box, parse_boxes};
use crate::error::{AppError, AppResult};

/// 视频轨的样本描述。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackInfo {
    /// 轨道标识
    pub track_id: u32,
    /// 是否为视频轨
    pub is_video: bool,
    /// 编码四字符码，如 `hvc1` / `avc1` / `mp4a`
    pub codec: String,
    /// 样本总数
    pub sample_count: u32,
    /// 每个样本的 [偏移, 大小]
    pub samples: Vec<(u64, u64)>,
    /// 每个样本的附加信息大小（CENC subsample 数量相关）
    pub aux_sizes: Vec<u8>,
    /// `senc` 中每个样本的 IV。
    ///
    /// ⚠️ **8 字节**——平台封装时就这么写的（现版 JS 按 `i * 8` 步长取），
    ///    不是 CENC 规范里的 16 字节。按 16 读会整体错位，解出来全是噪声。
    pub sample_ivs: Vec<[u8; 8]>,
    /// 原始 `stbl` 的绝对偏移与长度，重建 moov 时需要原样搬运
    pub stbl_offset: usize,
    pub stbl_size: usize,
    /// chunk 起始偏移（`stco` / `co64`）
    pub chunk_offsets: Vec<u64>,
    /// chunk 偏移是否为 64 位
    pub wide_offsets: bool,
    /// `stsc` 条目：`(first_chunk, samples_per_chunk)`，已按 first_chunk 升序
    pub stsc: Vec<(u32, u32)>,
}

impl TrackInfo {
    /// 样本总字节数。
    pub fn total_size(&self) -> u64 {
        self.samples.iter().map(|(_, s)| s).sum()
    }
}

/// 收集轨道信息。
pub fn collect_tracks(data: &[u8]) -> AppResult<Vec<TrackInfo>> {
    let moov = find_box(data, 0, data.len(), "moov")
        .ok_or_else(|| AppError::Decrypt("找不到 moov box".into()))?;

    let mut tracks = Vec::new();
    for trak in parse_boxes(data, moov.start, moov.start + moov.size) {
        if !trak.is("trak") {
            continue;
        }
        if let Some(info) = collect_track(data, trak.start, trak.start + trak.size) {
            tracks.push(info);
        }
    }
    Ok(tracks)
}

/// 收集单条轨道。
fn collect_track(data: &[u8], start: usize, end: usize) -> Option<TrackInfo> {
    let mut info = TrackInfo {
        stbl_offset: 0,
        stbl_size: 0,
        ..Default::default()
    };

    // track id
    if let Some(tkhd) = find_box(data, start, end, "tkhd") {
        let base = tkhd.start;
        // version(1) + flags(3)；version 1 用 64 位 id
        let version = data[base];
        if version == 1 && base + 28 <= tkhd.start + tkhd.size {
            info.track_id = u32::from_be_bytes([
                data[base + 20],
                data[base + 21],
                data[base + 22],
                data[base + 23],
            ]);
        } else if base + 20 <= tkhd.start + tkhd.size {
            info.track_id = u32::from_be_bytes([
                data[base + 12],
                data[base + 13],
                data[base + 14],
                data[base + 15],
            ]);
        }
    }

    // stbl 是样本表的根
    let stbl = find_box(data, start, end, "stbl")?;
    info.stbl_offset = stbl.start;
    info.stbl_size = stbl.size;

    // handler type：vide 表示视频轨
    if let Some(hdlr) = find_box(data, start, end, "hdlr") {
        let base = hdlr.start;
        if base + 12 <= hdlr.start + hdlr.size {
            info.is_video = &data[base + 8..base + 12] == b"vide";
        }
    }

    let stbl_end = stbl.start + stbl.size;
    for b in parse_boxes(data, stbl.start, stbl_end) {
        match b.kind_str().as_str() {
            "stsd" => info.codec = read_codec(data, b.start, b.size),
            "stsz" => read_stsz(data, b.start, b.size, &mut info),
            "stsc" => chunk_map::read_stsc(data, b.start, b.size, &mut info),
            "stco" => {
                info.wide_offsets = false;
                read_chunk_offsets(data, b.start, b.size, &mut info);
            }
            "co64" => {
                info.wide_offsets = true;
                read_chunk_offsets(data, b.start, b.size, &mut info);
            }
            "saiz" => cenc_info::read_saiz(data, b.start, b.size, &mut info),
            "senc" => cenc_info::read_senc(data, b.start, b.size, &mut info),
            _ => {}
        }
    }
    chunk_map::resolve_sample_offsets(&mut info);

    Some(info)
}

/// 从 `stsd` 读编码四字符码。
fn read_codec(data: &[u8], start: usize, size: usize) -> String {
    // 结构：version+flags(4) + entry_count(4) + [size(4) + format(4) + ...]
    // 所以 format 在载荷偏移 12 处（4+4+4）
    if start + 16 > start + size {
        return String::new();
    }
    String::from_utf8_lossy(&data[start + 12..start + 16]).to_string()
}

/// 从 `stsz` 读每个样本大小。
fn read_stsz(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    if size < 12 {
        return;
    }
    let sample_size = u32::from_be_bytes([
        data[start + 4],
        data[start + 5],
        data[start + 6],
        data[start + 7],
    ]);
    let count = u32::from_be_bytes([
        data[start + 8],
        data[start + 9],
        data[start + 10],
        data[start + 11],
    ]) as usize;
    info.sample_count = count as u32;

    info.samples = if sample_size != 0 {
        // 定长样本：偏移未知，等 stco 填
        vec![(0, u64::from(sample_size)); count]
    } else {
        let mut v = Vec::with_capacity(count);
        for i in 0..count {
            let at = start + 12 + i * 4;
            if at + 4 > start + size {
                break;
            }
            let s = u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
            v.push((0, u64::from(s)));
        }
        v
    };
}

/// 从 `stco` / `co64` 读 chunk 偏移。
///
/// 真实样本偏移需要 `stsc`（样本到 chunk 的映射）才能精确展开；
/// 本模块只用 chunk 起始偏移配合样本大小累加，足以定位连续存放的加密流。
fn read_chunk_offsets(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    if size < 8 {
        return;
    }
    let count = u32::from_be_bytes([
        data[start + 4],
        data[start + 5],
        data[start + 6],
        data[start + 7],
    ]) as usize;
    // stco 是 32 位偏移，co64 是 64 位；调用方已把 box 类型写进 info.wide_offsets
    let wide = info.wide_offsets;
    let entry = if wide { 8 } else { 4 };

    let mut chunks = Vec::with_capacity(count);
    for i in 0..count {
        let at = start + 8 + i * entry;
        if at + entry > start + size {
            break;
        }
        let off = if wide {
            u64::from_be_bytes([
                data[at],
                data[at + 1],
                data[at + 2],
                data[at + 3],
                data[at + 4],
                data[at + 5],
                data[at + 6],
                data[at + 7],
            ])
        } else {
            u64::from(u32::from_be_bytes([
                data[at],
                data[at + 1],
                data[at + 2],
                data[at + 3],
            ]))
        };
        chunks.push(off);
    }
    if chunks.is_empty() {
        return;
    }

    info.chunk_offsets = chunks;
    // 单 chunk 场景：直接按样本大小顺序累加展开
    if info.chunk_offsets.len() == 1 {
        let mut cursor = info.chunk_offsets[0];
        for s in info.samples.iter_mut() {
            s.0 = cursor;
            cursor += s.1;
        }
    }
}

#[cfg(test)]
#[path = "sample_table_tests.rs"]
mod tests;
