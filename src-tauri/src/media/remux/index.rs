//! 拼接产物的 `moov` 重建。
//!
//! 快速合并不解码也不编码，但**必须重写索引**：把 N 个各自从 t=0 开始的样本表
//! 拼成一条从 0 连续到全集末尾的时间轴。字节级顺序拼接做不到这件事——第 1 集的
//! `stco` 只覆盖第 1 集的 `mdat`，播放器读完就停，后面的集拿不到索引。
//!
//! 三条设计决定：
//!
//! - **每样本一个 chunk**（`stsc` 只有一条 `(1,1)`）。换来的是不必按源 `stsc`
//!   重新给 chunk 编号，也就不必处理「各集 stsc 不一致」这一整类分支。
//!   10 集约 2.2 万样本 → `stco` 约 88KB；只有合集超过 4GB 才升级成 `co64`。
//! - **交织顺序照抄源文件**。把一集内所有轨道的样本按源偏移归并排序后写出，
//!   就精确复刻了原文件的音视频排布，不需要对交错方式额外做假设。
//! - **`ctts` 的 offset 原样照搬**。合成时间 = 解码时间 + offset，而拼接后
//!   解码时间本身已随各集时长整体平移，两者同步平移后差值不变。再额外加一次
//!   起点偏移，会让每集首个 B 帧的错位逐集累积。
//!
//! `edts` / `mvex` / `udta` 一律丢弃：编辑列表指向各集自己的片段，合并后语义
//! 已不成立；时长改由样本表决定，对本地播放没有影响。

use crate::domain::mp4::r#box::{build_box, find_box, parse_boxes, BoxHeader};
use crate::domain::mp4::sample_table::{collect_track, TrackInfo};
use crate::error::{AppError, AppResult};

/// 一个输入文件里的一条轨道：解析结果 + 要原样搬进产物的 box。
pub struct SourceTrack {
    pub info: TrackInfo,
    /// `tkhd`：轨道标识与电影时基时长
    pub tkhd: Vec<u8>,
    /// `mdhd`：媒体时基时长
    pub mdhd: Vec<u8>,
    pub hdlr: Vec<u8>,
    /// `stsd`：样本描述，内含 `hvcC` / `avcC` / `esds`
    pub stsd: Vec<u8>,
    /// 视频轨的 `vmhd` 或音频轨的 `smhd`
    pub header: Vec<u8>,
    pub dinf: Vec<u8>,
}

/// 一个输入文件解析后的全部轨道，外加本集内的样本交织顺序。
pub struct SourceFile {
    /// 按 `moov` 里的顺序。合并要求逐文件一致，顺序不同就意味着轨道对不上。
    pub tracks: Vec<SourceTrack>,
    /// `(轨道序号, 该轨道内的样本序号)`，按源文件里的字节偏移升序。
    /// 写 `mdat` 时按这个顺序逐个拷贝，就复刻了原始排布。
    pub order: Vec<(usize, u32)>,
    /// 本集的 `mvhd`，用于核对电影时基是否一致
    pub mvhd: Vec<u8>,
}

/// 合并后一条轨道的索引素材。
#[derive(Debug)]
pub struct MergedTrack {
    pub tkhd: Vec<u8>,
    pub mdhd: Vec<u8>,
    pub hdlr: Vec<u8>,
    pub stsd: Vec<u8>,
    pub header: Vec<u8>,
    pub dinf: Vec<u8>,
    /// `stsz` 的逐样本大小
    pub sizes: Vec<u32>,
    /// `stts` 游程
    pub stts: Vec<(u32, u32)>,
    /// `ctts` 游程。空表示没有这张表，产物里也不写。
    pub ctts: Vec<(u32, i32)>,
    /// `stss` 同步样本号。`None` 表示没有这张表 = 全部样本都是关键帧。
    pub stss: Option<Vec<u32>>,
}

/// 整个合并产物的索引。
#[derive(Debug)]
pub struct MergeIndex {
    pub tracks: Vec<MergedTrack>,
    /// 已改好时长的 `mvhd`
    pub mvhd: Vec<u8>,
}

/// 解析一段 `moov` 字节，得到轨道、交织顺序与 `mvhd`。
pub fn read_source_file(moov: &[u8]) -> AppResult<SourceFile> {
    let header = parse_boxes(moov, 0, moov.len())
        .into_iter()
        .find(|b| b.is("moov"))
        .ok_or_else(|| AppError::Media("这段字节里没有 moov".into()))?;
    let children = parse_boxes(moov, header.start, header.start + header.size);

    let mvhd = children
        .iter()
        .find(|b| b.is("mvhd"))
        .map(|b| raw(moov, b))
        .ok_or_else(|| AppError::Media("moov 里没有 mvhd".into()))?;

    let mut tracks = Vec::new();
    for trak in children.iter().filter(|b| b.is("trak")) {
        let end = trak.start + trak.size;
        let info = collect_track(moov, trak.start, end)
            .ok_or_else(|| AppError::Media("有 trak 解析不出样本表".into()))?;

        // 偏移停在 0 = 样本表没解析出来。照这样的样本写 mdat 会把文件头当成
        // 媒体数据拷进产物，而产物照样「能打开」——只能在这里挡住。
        if info.samples.first().is_some_and(|(at, _)| *at == 0) {
            return Err(AppError::Media(
                "某条轨道的样本偏移没解析出来，拼接后会产出损坏文件".into(),
            ));
        }

        let pick = |kinds: &[&str]| -> AppResult<Vec<u8>> {
            for k in kinds {
                if let Some(b) = find_box(moov, trak.start, end, k) {
                    return Ok(raw(moov, &b));
                }
            }
            Err(AppError::Media(format!("轨道缺少 {} box", kinds[0])))
        };

        tracks.push(SourceTrack {
            tkhd: pick(&["tkhd"])?,
            mdhd: pick(&["mdhd"])?,
            hdlr: pick(&["hdlr"])?,
            stsd: pick(&["stsd"])?,
            header: pick(&["vmhd", "smhd"])?,
            dinf: pick(&["dinf"])?,
            info,
        });
    }

    if tracks.is_empty() {
        return Err(AppError::Media("moov 里没有轨道".into()));
    }

    Ok(SourceFile {
        order: interleave_order(&tracks),
        tracks,
        mvhd,
    })
}

/// 同一集内所有轨道的样本按源偏移归并。
fn interleave_order(tracks: &[SourceTrack]) -> Vec<(usize, u32)> {
    let mut all: Vec<(u64, usize, u32)> = Vec::new();
    for (t, track) in tracks.iter().enumerate() {
        for (i, (at, _)) in track.info.samples.iter().enumerate() {
            all.push((*at, t, i as u32));
        }
    }
    all.sort_by_key(|(at, t, i)| (*at, *t, *i));
    all.into_iter().map(|(_, t, i)| (t, i)).collect()
}

/// 把所有输入累加成一条完整的索引。
///
/// 轨道结构与电影时基必须逐文件一致。`codec_probe` 已经拦过编码参数，这里补的
/// 是它没管的部分：轨数、轨道类型顺序、`mvhd` 的 timescale。任一对不上都直接
/// 报错——硬拼出来的文件连索引都过不去。
pub fn plan(files: &[SourceFile]) -> AppResult<MergeIndex> {
    let Some(first) = files.first() else {
        return Err(AppError::Media("没有待合并的文件".into()));
    };
    check_structure(files)?;
    check_movie_timescale(files)?;

    // 只要有任一集带 ctts，产物就必须有：条目数必须等于样本数，缺一段就错位。
    let want_ctts = files
        .iter()
        .any(|f| f.tracks.iter().any(|t| !t.info.ctts.is_empty()));
    // 同理，stss 是「没有就全是关键帧」，有一集不带就意味着不能省。
    let want_stss = files
        .iter()
        .any(|f| f.tracks.iter().any(|t| t.info.stss.is_some()));

    let mut tracks = Vec::with_capacity(first.tracks.len());
    for t in 0..first.tracks.len() {
        let mut acc = Accumulator {
            tkhd: first.tracks[t].tkhd.clone(),
            mdhd: first.tracks[t].mdhd.clone(),
            hdlr: first.tracks[t].hdlr.clone(),
            stsd: first.tracks[t].stsd.clone(),
            header: first.tracks[t].header.clone(),
            dinf: first.tracks[t].dinf.clone(),
            sizes: Vec::new(),
            stts: Vec::new(),
            ctts: Vec::new(),
            stss: if want_stss { Some(Vec::new()) } else { None },
            media_duration: 0,
            movie_duration: 0,
        };

        for (episode, file) in files.iter().enumerate() {
            let track = &file.tracks[t];
            acc.push(&track.info, want_ctts, episode + 1);
            acc.movie_duration += duration_of(&track.tkhd, TKHD_DURATION);
        }

        acc.mdhd = set_duration(&acc.mdhd, MDHD_DURATION, acc.media_duration);
        acc.tkhd = set_duration(&acc.tkhd, TKHD_DURATION, acc.movie_duration);
        tracks.push(acc.finish());
    }

    let movie_duration: u64 = files
        .iter()
        .map(|f| duration_of(&f.mvhd, MVHD_DURATION))
        .sum();
    let mvhd = set_duration(&first.mvhd, MVHD_DURATION, movie_duration);

    Ok(MergeIndex { tracks, mvhd })
}

/// 轨道结构一致性。
///
/// 报错要指出**具体差在哪**：短剧各集是平台分别编码的，分辨率不统一是常态
/// （同一部剧里混着 1080p 与 720p），只说「编码参数不同」用户无从下手。
fn check_structure(files: &[SourceFile]) -> AppResult<()> {
    let base = &files[0].tracks;
    for (k, file) in files.iter().enumerate().skip(1) {
        let episode = k + 1;
        if file.tracks.len() != base.len() {
            return Err(structure_error(
                episode,
                &format!(
                    "有 {} 条轨道，第 1 集有 {} 条",
                    file.tracks.len(),
                    base.len()
                ),
            ));
        }
        for (t, (a, b)) in base.iter().zip(&file.tracks).enumerate() {
            if a.info.is_video != b.info.is_video {
                return Err(structure_error(
                    episode,
                    &format!("第 {} 条轨道的音视频类型与第 1 集不同", t + 1),
                ));
            }
            if a.info.codec != b.info.codec {
                return Err(structure_error(
                    episode,
                    &format!(
                        "第 {} 条轨道的编码是 {}，第 1 集是 {}",
                        t + 1,
                        b.info.codec,
                        a.info.codec
                    ),
                ));
            }
            if a.info.width != b.info.width || a.info.height != b.info.height {
                return Err(structure_error(
                    episode,
                    &format!(
                        "第 {} 条轨道的分辨率是 {}x{}，第 1 集是 {}x{}",
                        t + 1,
                        b.info.width,
                        b.info.height,
                        a.info.width,
                        a.info.height
                    ),
                ));
            }
            if a.info.channels != b.info.channels || a.info.sample_rate != b.info.sample_rate {
                return Err(structure_error(
                    episode,
                    &format!(
                        "第 {} 条轨道的音频是 {}ch/{}Hz，第 1 集是 {}ch/{}Hz",
                        t + 1,
                        b.info.channels,
                        b.info.sample_rate,
                        a.info.channels,
                        a.info.sample_rate
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// 电影时基必须一致，否则各集 `tkhd` 的时长不能直接相加。
fn check_movie_timescale(files: &[SourceFile]) -> AppResult<()> {
    let base = timescale_of(&files[0].mvhd, MVHD_TIMESCALE);
    for (k, file) in files.iter().enumerate().skip(1) {
        if timescale_of(&file.mvhd, MVHD_TIMESCALE) != base {
            return Err(structure_error(
                k + 1,
                "的 mvhd 时基与第 1 集不同，时长无法相加",
            ));
        }
    }
    Ok(())
}

/// 逐集累加一条轨道的索引素材。
struct Accumulator {
    tkhd: Vec<u8>,
    mdhd: Vec<u8>,
    hdlr: Vec<u8>,
    stsd: Vec<u8>,
    header: Vec<u8>,
    dinf: Vec<u8>,
    sizes: Vec<u32>,
    stts: Vec<(u32, u32)>,
    ctts: Vec<(u32, i32)>,
    stss: Option<Vec<u32>>,
    /// 媒体时基下的累计时长
    media_duration: u64,
    /// 电影时基下的累计时长
    movie_duration: u64,
}

impl Accumulator {
    fn push(&mut self, info: &TrackInfo, want_ctts: bool, episode: usize) {
        let count = info.samples.len();
        if count == 0 {
            return;
        }
        let base_index = self.sizes.len() as u32;

        self.sizes
            .extend(info.samples.iter().map(|(_, size)| *size as u32));

        if info.stts.is_empty() {
            // 没有 stts 的畸形轨道：按每样本一个单位时长补一条，
            // 否则后面各集的样本会挤在 t=0 之前
            push_run(&mut self.stts, count as u32, 1);
        } else {
            for &(n, delta) in &info.stts {
                push_run(&mut self.stts, n, delta);
            }
        }

        if want_ctts {
            if info.ctts.is_empty() {
                push_run(&mut self.ctts, count as u32, 0);
            } else {
                for &(n, offset) in &info.ctts {
                    // 原样照搬，不加各集起点：合成时间与解码时间同步平移，
                    // 差值（也就是 offset）不变
                    push_run(&mut self.ctts, n, offset);
                }
            }
        }

        if let Some(sync) = self.stss.as_mut() {
            match &info.stss {
                Some(nums) => sync.extend(nums.iter().map(|n| n + base_index)),
                // 没有 stss = 全部样本都是关键帧，整段并进去
                None => sync.extend((base_index + 1)..=(base_index + count as u32)),
            }
        }

        self.media_duration += track_duration(info);
        if self.media_duration == 0 {
            log::warn!("[Remux] 第 {episode} 集的轨道时长为 0，合并后时间轴可能不连续");
        }
    }

    fn finish(mut self) -> MergedTrack {
        // stss 攒下来还是空的，说明没有任何一集是关键帧表非空——那就不写
        if self.stss.as_ref().is_some_and(Vec::is_empty) {
            self.stss = None;
        }
        MergedTrack {
            tkhd: self.tkhd,
            mdhd: self.mdhd,
            hdlr: self.hdlr,
            stsd: self.stsd,
            header: self.header,
            dinf: self.dinf,
            sizes: self.sizes,
            stts: self.stts,
            ctts: self.ctts,
            stss: self.stss,
        }
    }
}

/// 追加一段游程；与上一段 delta 相同就并进去，免得游程表按集数线性膨胀。
fn push_run<T: PartialEq>(runs: &mut Vec<(u32, T)>, count: u32, delta: T) {
    if count == 0 {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.1 == delta => last.0 += count,
        _ => runs.push((count, delta)),
    }
}

/// 一条轨道的时长，以 `stts` 累计值为准——它才真正定义样本时间轴。
/// 没有 `stts` 的畸形轨道退回 `mdhd`。
fn track_duration(info: &TrackInfo) -> u64 {
    let sum: u64 = info
        .stts
        .iter()
        .map(|&(n, d)| u64::from(n) * u64::from(d))
        .sum();
    if sum > 0 {
        sum
    } else {
        info.media_duration
    }
}

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
    if wide {
        16
    } else {
        8
    }
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
    trak.extend(build_box(b"mdia", &mdia));
    build_box(b"trak", &trak)
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

// ── FullBox 字段读写 ─────────────────────────────────────────────
// 下面的偏移都相对 box 的**载荷**（version 之后的第一个字节）。
// mvhd / mdhd / tkhd 都是「时间基 + 时长」相邻的同形结构，只有 tkhd 前面
// 多一个 track_ID 与一个 reserved。

const MVHD_TIMESCALE: (usize, usize) = (12, 20);
const MVHD_DURATION: (usize, usize) = (16, 24);
const MDHD_DURATION: (usize, usize) = (16, 24);
const TKHD_DURATION: (usize, usize) = (20, 28);

/// box 头长度。[`raw`] 取的是**整个 box**，字段偏移是相对载荷的，两者要补上它。
const BOX_HEADER: usize = 8;

/// version 0 取前一个、version 1 取后一个，再补上 box 头。
fn field_at(at: (usize, usize), wide: bool) -> usize {
    (if wide { at.1 } else { at.0 }) + BOX_HEADER
}

fn is_wide(data: &[u8]) -> bool {
    data.len() > 8 && data[BOX_HEADER] == 1
}

/// 改写时长字段：version 0 是 32 位、version 1 是 64 位。
fn set_duration(data: &[u8], at: (usize, usize), duration: u64) -> Vec<u8> {
    let mut out = data.to_vec();
    if out.len() < 8 {
        return out;
    }
    let at = field_at(at, is_wide(&out));
    if is_wide(&out) {
        if at + 8 > out.len() {
            return out;
        }
        out[at..at + 8].copy_from_slice(&duration.to_be_bytes());
    } else {
        if at + 4 > out.len() {
            return out;
        }
        let v = duration.min(u64::from(u32::MAX)) as u32;
        out[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn duration_of(data: &[u8], at: (usize, usize)) -> u64 {
    if data.len() < 8 {
        return 0;
    }
    let wide = is_wide(data);
    let at = field_at(at, wide);
    if wide {
        if at + 8 > data.len() {
            return 0;
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&data[at..at + 8]);
        u64::from_be_bytes(b)
    } else {
        u64::from(read_u32(data, at))
    }
}

fn timescale_of(data: &[u8], at: (usize, usize)) -> u32 {
    if data.len() < 8 {
        return 0;
    }
    read_u32(data, field_at(at, is_wide(data)))
}

fn read_u32(data: &[u8], at: usize) -> u32 {
    if at + 4 > data.len() {
        return 0;
    }
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

/// 把一个 box 连头带尾原样取出。
fn raw(data: &[u8], header: &BoxHeader) -> Vec<u8> {
    let from = header.start - header.header_size;
    data[from..header.start + header.size].to_vec()
}

fn structure_error(episode: usize, what: &str) -> AppError {
    AppError::Media(format!("第 {episode} 集{what}，快速合并会产出损坏文件"))
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
