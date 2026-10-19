//! 合并计划的素材与校验：解析各输入的 `moov`、逐集累加出合并后的样本表。
//!
//! 写出侧（`moov` 字节组装）见 [`super::moov`]。

use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::{BoxHeader, find_box, parse_boxes};
use hongguo_video::sample_table::{TrackInfo, collect_track};

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
    // 逐集「非视频轨应收累计时长」：音画不同步的根治在样本表层（见
    // [`alignment_targets`]），每集收尾把差值补进末样本
    let targets = alignment_targets(files);
    for (t, targets_of_track) in targets.iter().enumerate() {
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
            if let Some(target) = targets_of_track[episode] {
                acc.lock_tail_to(target, episode + 1);
            }
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

/// 逐集「每条非视频轨的应收累计时长」（本轨媒体时基下的 tick 数）。
///
/// 短剧各集是平台分别编码的，每集音/视频轨时长有毫秒级出入；按轨独立
/// 累加会让差值逐集累积，N 集后声音明显偏离画面（issue #11「快速合并
/// 图音不同步，声音比画面慢很多」）。以**视频轨为主轴**：把每集的累计
/// 视频时长换算到本轨时基定为本轨应收累计，收尾差值由
/// [`Accumulator::lock_tail_to`] 补齐。没有视频轨（纯音频拼接）无从对齐，
/// 全部返回 None 跳过。
fn alignment_targets(files: &[SourceFile]) -> Vec<Vec<Option<i64>>> {
    let Some(master) = files[0].tracks.iter().position(|t| t.info.is_video) else {
        return vec![vec![None; files.len()]; files[0].tracks.len()];
    };
    let master_ts = f64::from(mdhd_timescale(&files[0].tracks[master].mdhd).max(1));
    let mut master_cum_secs = 0.0;
    let master_cum: Vec<f64> = files
        .iter()
        .map(|file| {
            master_cum_secs += track_duration(&file.tracks[master].info) as f64 / master_ts;
            master_cum_secs
        })
        .collect();
    files[0]
        .tracks
        .iter()
        .enumerate()
        .map(|(t, track)| {
            let ts = f64::from(mdhd_timescale(&track.mdhd).max(1));
            master_cum
                .iter()
                .map(|secs| (secs * ts).round() as i64)
                .map(|target| (t != master).then_some(target))
                .collect()
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
    /// 媒体时基下的累计时长
    media_duration: u64,
    /// 电影时基下的累计时长
    movie_duration: u64,
}

impl Accumulator {
    /// 把累计时长锁到目标 tick（该轨媒体时基）：差值补进**最后一个样本**
    /// 的 stts 时长——只把末位游程拆出末样本单独成段，样本数与逐样本的
    /// `sizes`/`ctts`/`stss` 完全不受影响。
    ///
    /// 每集收尾调一次，音/视频两轨的时间轴逐集锁齐：源各集独立编码带来
    /// 的毫秒级时长差（含被丢弃的 elst 未跳过的 AAC priming）不再逐集累积
    /// （issue #11「快速合并音画不同步」）。补正等价样本级静音/裁尾，对
    /// 播放器表现为该样本之后的时间轴空档/提前，毫秒级不可感。
    fn lock_tail_to(&mut self, target: i64, episode: usize) {
        let pad = target - self.media_duration as i64;
        if pad == 0 {
            return;
        }
        let Some(&(count, delta)) = self.stts.last() else {
            return;
        };
        // 末样本时长减不到 1 tick（音频远长于视频的病态源）：钳在 1，
        // 残差留 warn——宁留本集毫秒级偏差也不产出非法的 0 时长样本
        let shifted = delta as i64 + pad;
        if shifted < 1 {
            log::warn!(
                "[Remux] 第 {episode} 集收尾对齐差 {pad} tick 超出末样本时长 {delta}，\
                 本集残留音画偏差（源文件音视频轨严重不齐）"
            );
        }
        let applied = shifted.max(1) - delta as i64;
        if applied == 0 {
            return;
        }
        let new_delta = (delta as i64 + applied) as u32;
        if count > 1 {
            self.stts.last_mut().expect("刚读过末游程").0 = count - 1;
        } else {
            self.stts.pop();
        }
        push_run(&mut self.stts, 1, new_delta);
        self.media_duration = (self.media_duration as i64 + applied) as u64;
    }

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

/// 把一个 box 连头带尾原样取出。
fn raw(data: &[u8], header: &BoxHeader) -> Vec<u8> {
    let from = header.start - header.header_size;
    data[from..header.start + header.size].to_vec()
}

fn structure_error(episode: usize, what: &str) -> AppError {
    AppError::Media(format!("第 {episode} 集{what}，快速合并会产出损坏文件"))
}
