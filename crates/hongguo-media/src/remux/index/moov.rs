//! `moov` 写出：把 [`super::plan`] 产出的索引素材拼成产物的 `moov` 字节。

use hongguo_video::r#box::build_box;

use super::{Edit, MergeIndex, MergedTrack};

/// 生成合并后的 `moov`。
///
/// `offsets` 是每条轨道每个样本在输出文件里的绝对偏移。调用方要先用占位偏移
/// 量一次长度、算出 `mdat` 起点，再带着真实偏移调一次——两次结果长度相同。
pub fn build_moov(index: &MergeIndex, offsets: &[Vec<u64>], wide: bool) -> Vec<u8> {
    let mut payload = index.mvhd.clone();
    for (t, track) in index.tracks.iter().enumerate() {
        payload.extend(build_trak(track, &offsets[t], wide));
    }
    build_box(b"moov", &payload)
}

/// `mdat` 的 box 头长度。载荷放不下 32 位时用 largesize，多 8 字节。
///
/// 与 [`needs_wide_offsets`] 同源：`stco` 存不下偏移时，`mdat` 的长度同样
/// 存不下。两个都升到 64 位，才不会出现「索引说得出、box 装不下」。
pub fn mdat_header_len(wide: bool) -> usize {
    if wide { 16 } else { 8 }
}

/// `mdat` 载荷（媒体数据）的起始偏移。
pub fn mdat_data_start(ftyp_len: usize, moov_len: usize, wide: bool) -> u64 {
    (ftyp_len + moov_len + mdat_header_len(wide)) as u64
}

/// 偏移是否必须用 64 位：合集超过 4GB 时 `stco` 的 32 位存不下。
pub fn needs_wide_offsets(total_payload: u64) -> bool {
    total_payload > u64::from(u32::MAX)
}

fn build_trak(track: &MergedTrack, offsets: &[u64], wide: bool) -> Vec<u8> {
    let mut minf = track.header.clone();
    minf.extend(track.dinf.clone());
    minf.extend(build_stbl(track, offsets, wide));

    let mut mdia = track.mdhd.clone();
    mdia.extend(track.hdlr.clone());
    mdia.extend(build_box(b"minf", &minf));

    let mut trak = track.tkhd.clone();
    trak.extend(edts_box(&track.edits));
    trak.extend(build_box(b"mdia", &mdia));
    build_box(b"trak", &trak)
}

/// 逐集 presentation 映射。`tkhd` 的时长是各段之和，`mdhd` 仍是媒体时长；
/// 播放器靠这张表跳过每集开头的 AAC priming / 视频起始偏移。
///
/// 统一写 version 1（64 位时长/有符号 media_time），避免合集超长时截断。
/// 空 edit 的 `media_time = -1` 按位写回全 1。
fn edts_box(edits: &[Edit]) -> Vec<u8> {
    if edits.is_empty() {
        return Vec::new();
    }
    let mut p = vec![1u8, 0, 0, 0]; // version 1 + flags
    p.extend((edits.len() as u32).to_be_bytes());
    for edit in edits {
        p.extend(edit.segment_duration.to_be_bytes());
        p.extend((edit.media_time as u64).to_be_bytes());
        p.extend(edit.rate.to_be_bytes());
    }
    build_box(b"edts", &build_box(b"elst", &p))
}

fn build_stbl(track: &MergedTrack, offsets: &[u64], wide: bool) -> Vec<u8> {
    let mut p = track.stsd.clone();
    p.extend(stts_box(&track.stts));
    if !track.ctts.is_empty() {
        p.extend(ctts_box(&track.ctts));
    }
    if let Some(sync) = &track.stss {
        p.extend(stss_box(sync));
    }
    p.extend(stsc_box());
    p.extend(stsz_box(&track.sizes));
    p.extend(stco_box(offsets, wide));
    build_box(b"stbl", &p)
}

fn stts_box(runs: &[(u32, u32)]) -> Vec<u8> {
    let mut p = 0u32.to_be_bytes().to_vec();
    p.extend((runs.len() as u32).to_be_bytes());
    for &(n, d) in runs {
        p.extend(n.to_be_bytes());
        p.extend(d.to_be_bytes());
    }
    build_box(b"stts", &p)
}

/// 统一发 version 1（有符号）。源文件可能混用 0/1 两个版本，重写成一种，
/// 才不会在拼接处出现同一份字节被前后解释成不同符号的情况。
///
/// FullBox 的 version 是**第 1 个字节**、flags 占后 3 个——写成
/// `1u32.to_be_bytes()` 会得到 version=0 / flags=1，符号全反。
fn ctts_box(runs: &[(u32, i32)]) -> Vec<u8> {
    let mut p = vec![1u8, 0, 0, 0]; // version 1 + flags
    p.extend((runs.len() as u32).to_be_bytes());
    for &(n, o) in runs {
        p.extend(n.to_be_bytes());
        p.extend((o as u32).to_be_bytes());
    }
    build_box(b"ctts", &p)
}

fn stss_box(sync: &[u32]) -> Vec<u8> {
    let mut p = 0u32.to_be_bytes().to_vec();
    p.extend((sync.len() as u32).to_be_bytes());
    for n in sync {
        p.extend(n.to_be_bytes());
    }
    build_box(b"stss", &p)
}

/// 每 chunk 一个样本，所以表里只有一条条目。
fn stsc_box() -> Vec<u8> {
    let mut p = 0u32.to_be_bytes().to_vec();
    p.extend(1u32.to_be_bytes()); // entry_count
    p.extend(1u32.to_be_bytes()); // first_chunk
    p.extend(1u32.to_be_bytes()); // samples_per_chunk
    p.extend(1u32.to_be_bytes()); // sample_description_index
    build_box(b"stsc", &p)
}

fn stsz_box(sizes: &[u32]) -> Vec<u8> {
    let mut p = 0u32.to_be_bytes().to_vec(); // version + flags
    p.extend(0u32.to_be_bytes()); // sample_size = 0：逐样本给
    p.extend((sizes.len() as u32).to_be_bytes());
    for s in sizes {
        p.extend(s.to_be_bytes());
    }
    build_box(b"stsz", &p)
}

fn stco_box(offsets: &[u64], wide: bool) -> Vec<u8> {
    let mut p = 0u32.to_be_bytes().to_vec();
    p.extend((offsets.len() as u32).to_be_bytes());
    if wide {
        for o in offsets {
            p.extend(o.to_be_bytes());
        }
        build_box(b"co64", &p)
    } else {
        for o in offsets {
            p.extend((*o as u32).to_be_bytes());
        }
        build_box(b"stco", &p)
    }
}
