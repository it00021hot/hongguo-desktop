//! MP4 写出层：`moov` 组装与 `mdat` 布局。
//!
//! 两条产出路径共用这一份：
//! - [`crate::remux`]：流复制拼接——索引从源文件搬来，只改时长与偏移；
//! - [`crate::mux`]：编码后重新封装——索引由编码输出构造。
//!
//! 产物布局统一是 `[ftyp][moov][mdat]`（faststart）：`moov` 在前，播放器读文件头
//! 就能拿到时长与样本表，不必先跳过整个媒体数据。
//!
//! 样本表（`stbl`）里的 `ctts` 是**有符号**的（version 1）：B 帧的合成时间与解码
//! 时间之差可以为负。这一层是本仓 B 帧能力的根基——`ctts` 是一等公民，不是补丁。

use hongguo_video::r#box::build_box;

/// 正常播放速率（16.16 定点）。
pub const EDIT_RATE_ONE: i32 = 0x0001_0000;

/// 一条 `edts/elst` 条目：从源媒体时间轴映射到 presentation 时间轴。
///
/// `media_time` 是 media timescale；`segment_duration` 是 movie timescale。
/// 两者时基不同，不能直接相加。`-1` 是空 edit（只占 presentation 时长）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit {
    pub segment_duration: u64,
    pub media_time: i64,
    pub rate: i32,
}

/// 一条轨道的索引素材：组装 `trak` 所需的全部字节与表。
#[derive(Debug)]
pub struct TrackSpec {
    /// `tkhd`：轨道标识与电影时基时长
    pub tkhd: Vec<u8>,
    /// `mdhd`：媒体时基时长
    pub mdhd: Vec<u8>,
    /// `hdlr`：`vide` / `soun`
    pub hdlr: Vec<u8>,
    /// `stsd`：样本描述，内含 `avcC` / `esds`
    pub stsd: Vec<u8>,
    /// `minf` 里的 `vmhd`（视频）或 `smhd`（音频）
    pub header: Vec<u8>,
    /// `minf` 里的 `dinf`
    pub dinf: Vec<u8>,
    /// `stsz` 的逐样本大小
    pub sizes: Vec<u32>,
    /// `stts` 游程
    pub stts: Vec<(u32, u32)>,
    /// `ctts` 游程。空表示没有这张表，产物里也不写。
    pub ctts: Vec<(u32, i32)>,
    /// `stss` 同步样本号。`None` 表示没有这张表 = 全部样本都是关键帧。
    pub stss: Option<Vec<u32>>,
    /// `edts/elst`：presentation 映射。至少一条（无源 edit 的补默认段）。
    pub edits: Vec<Edit>,
}

/// 追加一段游程；与上一段取值相同就并进去，免得游程表按样本数线性膨胀。
pub fn push_run<T: PartialEq>(runs: &mut Vec<(u32, T)>, count: u32, value: T) {
    if count == 0 {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.1 == value => last.0 += count,
        _ => runs.push((count, value)),
    }
}

/// 生成 `moov`。
///
/// `offsets` 是每条轨道每个样本在输出文件里的绝对偏移。调用方要先用占位偏移
/// 量一次长度、算出 `mdat` 起点，再带着真实偏移调一次——两次结果长度相同。
pub fn build_moov(mvhd: &[u8], tracks: &[TrackSpec], offsets: &[Vec<u64>], wide: bool) -> Vec<u8> {
    let mut payload = mvhd.to_vec();
    for (t, track) in tracks.iter().enumerate() {
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

/// 偏移是否必须用 64 位：文件超过 4GB 时 `stco` 的 32 位存不下。
pub fn needs_wide_offsets(total_payload: u64) -> bool {
    total_payload > u64::from(u32::MAX)
}

/// `mdat` 的 box 头。载荷装不下 32 位时用 largesize。
///
/// 返回的**长度就是真正写出去的字节数**——固定长度的数组会在窄偏移时多写出
/// 8 个零字节，所有样本偏移随之偏小。
pub fn mdat_header(total_payload: u64, wide: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    if wide {
        out.extend(1u32.to_be_bytes());
        out.extend(b"mdat");
        out.extend(total_payload.to_be_bytes());
    } else {
        let total = (mdat_header_len(false) as u64 + total_payload) as u32;
        out.extend(total.to_be_bytes());
        out.extend(b"mdat");
    }
    out
}

fn build_trak(track: &TrackSpec, offsets: &[u64], wide: bool) -> Vec<u8> {
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

/// 逐段 presentation 映射。`tkhd` 的时长是各段之和，`mdhd` 仍是媒体时长；
/// 播放器靠这张表跳过每段开头的 AAC priming / 视频起始偏移。
///
/// 统一写 version 1（64 位时长/有符号 media_time），避免文件超长时截断。
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

fn build_stbl(track: &TrackSpec, offsets: &[u64], wide: bool) -> Vec<u8> {
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
