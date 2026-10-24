//! 合并计划的素材与校验：解析各输入的 `moov`、逐集累加出合并后的样本表。
//!
//! 写出侧（`moov` 字节组装）见 [`crate::mp4`]。

use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::{BoxHeader, find_box, parse_boxes};
use hongguo_video::sample_table::{TrackInfo, collect_track};

use crate::mp4::{EDIT_RATE_ONE, Edit, TrackSpec, push_run};

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
    /// 源 `trak/edts/elst` 的全部条目。空 = 没有 edit list，按 media 起点补齐。
    pub edits: Vec<Edit>,
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

/// 整个合并产物的索引。
#[derive(Debug)]
pub struct MergeIndex {
    pub tracks: Vec<TrackSpec>,
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
            edits: read_edits(moov, trak),
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

/// 读一条轨道的 `edts/elst`。缺失/畸形返回空，调用方补默认 edit。
///
/// `elst` 的 `media_time` 是 media timescale、`segment_duration` 是 movie
/// timescale；这里只做原样解析，跨集平移在 [`plan`] 里按 media ticks 累加。
fn read_edits(data: &[u8], trak: &BoxHeader) -> Vec<Edit> {
    let Some(edts) = find_box(data, trak.start, trak.start + trak.size, "edts") else {
        return Vec::new();
    };
    let Some(elst) = find_box(data, edts.start, edts.start + edts.size, "elst") else {
        return Vec::new();
    };
    let end = elst.start + elst.size;
    if elst.start + 8 > end {
        return Vec::new();
    }
    let version = data[elst.start];
    let count = read_u32(data, elst.start + 4) as usize;
    let entry = if version == 1 { 20 } else { 12 };
    let mut edits = Vec::new();
    for i in 0..count {
        let at = elst.start + 8 + i * entry;
        if at + entry > end {
            break;
        }
        let (segment_duration, media_time, rate) = if version == 1 {
            (
                read_u64(data, at),
                read_u64(data, at + 8) as i64,
                read_u32(data, at + 16) as i32,
            )
        } else {
            (
                u64::from(read_u32(data, at)),
                i64::from(read_u32(data, at + 4) as i32),
                read_u32(data, at + 8) as i32,
            )
        };
        edits.push(Edit {
            segment_duration,
            media_time,
            rate,
        });
    }
    edits
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

    let movie_ts = timescale_of(&first.mvhd, MVHD_TIMESCALE).max(1);
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
            edits: Vec::new(),
            media_duration: 0,
            movie_duration: 0,
        };

        for (episode, file) in files.iter().enumerate() {
            let track = &file.tracks[t];
            // 本集 media 起点：源 elst 的 media_time 都是相对**本集** media 的，
            // 拼到合并轨上要整体平移到本集起点。
            let media_start = acc.media_duration as i64;
            let episode_edits = compose_episode_edits(track, media_start, movie_ts);
            acc.movie_duration += episode_edits
                .iter()
                .map(|e| e.segment_duration)
                .sum::<u64>();
            acc.edits.extend(episode_edits);
            acc.push(&track.info, want_ctts, episode + 1);
        }

        acc.mdhd = set_duration(&acc.mdhd, MDHD_DURATION, acc.media_duration);
        acc.tkhd = set_duration(&acc.tkhd, TKHD_DURATION, acc.movie_duration);
        tracks.push(acc.finish());
    }

    // 电影时长取各轨 presentation 的最大值：源 mvhd 是「各集自己的上界」，
    // 逐集相加会把每集内部的音频/视频毫秒差也加进来（实测 10 集多出 76ms
    // 尾部空档）。edit list 的段时长之和才是本轨真实的 presentation 结束点。
    let movie_duration = tracks
        .iter()
        .map(|t| duration_of(&t.tkhd, TKHD_DURATION))
        .max()
        .unwrap_or(0);
    let mvhd = set_duration(&first.mvhd, MVHD_DURATION, movie_duration);

    Ok(MergeIndex { tracks, mvhd })
}

/// 把一集的源 edit 平移到合并轨的 media 起点；源没有 elst 时补一条默认段。
///
/// 默认段让每条轨在每个集边界都有明确的 presentation 映射，播放器不会拿
/// 「上一条 edit 继续往后推」这种隐式行为解释拼接处。`segment_duration` 为 0
/// 的畸形 edit 回落到 `tkhd` 时长，避免写出零长段。
fn compose_episode_edits(track: &SourceTrack, media_start: i64, movie_ts: u32) -> Vec<Edit> {
    let segment_fallback = {
        let by_tkhd = duration_of(&track.tkhd, TKHD_DURATION);
        if by_tkhd > 0 {
            by_tkhd
        } else {
            let media_ts = u64::from(mdhd_timescale(&track.mdhd).max(1));
            (track_duration(&track.info) * u64::from(movie_ts) / media_ts).max(1)
        }
    };
    if track.edits.is_empty() {
        return vec![Edit {
            segment_duration: segment_fallback,
            media_time: media_start,
            rate: EDIT_RATE_ONE,
        }];
    }
    track
        .edits
        .iter()
        .map(|edit| Edit {
            segment_duration: if edit.segment_duration > 0 {
                edit.segment_duration
            } else {
                segment_fallback
            },
            media_time: if edit.media_time >= 0 {
                media_start.saturating_add(edit.media_time)
            } else {
                edit.media_time
            },
            rate: if edit.rate == 0 {
                EDIT_RATE_ONE
            } else {
                edit.rate
            },
        })
        .collect()
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
            // 逐集时长相加的前提：同一轨的媒体时基一致（stts 的 delta 都是
            // 「各自 mdhd 时基下的 tick」，混着加就是悄悄改速率）
            if mdhd_timescale(&a.mdhd) != mdhd_timescale(&b.mdhd) {
                return Err(structure_error(
                    episode,
                    &format!(
                        "第 {} 条轨道的媒体时基是 {}，第 1 集是 {}",
                        t + 1,
                        mdhd_timescale(&b.mdhd),
                        mdhd_timescale(&a.mdhd)
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
    /// 逐集 presentation 映射，最终写进 `edts/elst`
    edits: Vec<Edit>,
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

    fn finish(mut self) -> TrackSpec {
        // stss 攒下来还是空的，说明没有任何一集是关键帧表非空——那就不写
        if self.stss.as_ref().is_some_and(Vec::is_empty) {
            self.stss = None;
        }
        TrackSpec {
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
            edits: self.edits,
        }
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
    if sum > 0 { sum } else { info.media_duration }
}

// ── FullBox 字段读写 ─────────────────────────────────────────────
// 下面的偏移都相对 box 的**载荷**（version 之后的第一个字节）。
// mvhd / mdhd / tkhd 都是「时间基 + 时长」相邻的同形结构，只有 tkhd 前面
// 多一个 track_ID 与一个 reserved。

const MVHD_TIMESCALE: (usize, usize) = (12, 20);
pub(super) const MVHD_DURATION: (usize, usize) = (16, 24);
pub(super) const MDHD_DURATION: (usize, usize) = (16, 24);
/// `mdhd` 的媒体时基字段（与 `mvhd` 同形：version/flags + creation +
/// modification + timescale，载荷偏移 12）。
const MDHD_TIMESCALE: (usize, usize) = (12, 20);
pub(super) const TKHD_DURATION: (usize, usize) = (20, 28);

/// 一条轨道的媒体时基（逐集相加与音画锁齐都按它换算）。
fn mdhd_timescale(mdhd: &[u8]) -> u32 {
    timescale_of(mdhd, MDHD_TIMESCALE)
}

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

pub(super) fn duration_of(data: &[u8], at: (usize, usize)) -> u64 {
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

fn read_u64(data: &[u8], at: usize) -> u64 {
    if at + 8 > data.len() {
        return 0;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&data[at..at + 8]);
    u64::from_be_bytes(b)
}

/// 把一个 box 连头带尾原样取出。
fn raw(data: &[u8], header: &BoxHeader) -> Vec<u8> {
    let from = header.start - header.header_size;
    data[from..header.start + header.size].to_vec()
}

fn structure_error(episode: usize, what: &str) -> AppError {
    AppError::Media(format!("第 {episode} 集{what}，快速合并会产出损坏文件"))
}
