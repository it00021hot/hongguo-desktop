//! 可追加的 H.264 fMP4。首个封闭 GOP 写成独立片段后文件即可从 0 解起，后续 GOP 只追加 `moof+mdat`。
//! `moov` 里的时长是整段时间轴，字节长度随片段增长。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::{build_box, parse_boxes};

use super::avcc;
use super::track::{audio_stsd, dinf, hdlr, mdhd, mvhd, smhd, tkhd, video_stsd, vmhd};
use super::{AudioTrack, MEDIA_TIMESCALE, MOVIE_TIMESCALE};

const VIDEO_TRACK: u32 = 1;
const AUDIO_TRACK: u32 = 2;
const TFHD_DEFAULT_BASE_IS_MOOF: u32 = 0x020000;
const TRUN_FLAGS: u32 = 0x0001 | 0x0100 | 0x0200 | 0x0400 | 0x0800;
const SAMPLE_SYNC: u32 = 0x0200_0000;
const SAMPLE_NON_SYNC: u32 = 0x0101_0000;
const NON_SYNC_BIT: u32 = 0x0001_0000;

struct AudioHeader {
    channels: u16,
    sample_rate: u32,
    esds: Option<Vec<u8>>,
}

/// 已经开始写、并且还会继续追加的 H.264 文件。
pub struct GrowingH264 {
    file: File,
    sequence: u32,
    timeline_secs: f64,
    width: usize,
    height: usize,
    audio: Option<AudioHeader>,
    started: bool,
}

/// 一次追加后文件上能看到的视频时间。
pub struct FragmentVideo {
    pub pts: Vec<f64>,
    pub first_is_sync: bool,
    pub fragment_bases: Vec<u64>,
}

impl GrowingH264 {
    pub fn create(
        path: &Path,
        width: usize,
        height: usize,
        audio: Option<&AudioTrack>,
        timeline_secs: f64,
    ) -> AppResult<Self> {
        if width == 0 || height == 0 || timeline_secs <= 0.0 {
            return Err(AppError::Media("片段封装缺少分辨率或时长".into()));
        }
        if let Some(parent) = path.parent() {
            hongguo_common::offline_qa::qa_create_dir_all(parent)
                .map_err(|e| AppError::Io(e.to_string()))?;
        }
        let file =
            hongguo_common::offline_qa::qa_create(path).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(Self {
            file,
            sequence: 1,
            timeline_secs,
            width,
            height,
            audio: audio.map(|track| AudioHeader {
                channels: track.channels,
                sample_rate: track.sample_rate,
                esds: track.esds.clone(),
            }),
            started: false,
        })
    }

    /// 写入一个 GOP 以及落在它时间范围内的音频。`video` 的第一帧必须是关键帧。
    pub fn push(
        &mut self,
        video: &[(f64, Vec<u8>, bool)],
        audio: &[(f64, Vec<u8>)],
    ) -> AppResult<u64> {
        if video.is_empty() || (!self.started && !video[0].2) {
            return Err(AppError::Media("片段缺少同步样本".into()));
        }
        let timed = time_samples(video, true)?;
        let audio_timed = time_samples(
            &audio
                .iter()
                .map(|(pts, data)| (*pts, data.clone(), true))
                .collect::<Vec<_>>(),
            false,
        )?;
        if !self.started {
            let init = init_segment(
                self.width,
                self.height,
                self.audio.as_ref(),
                &video[0].1,
                self.timeline_secs,
            )?;
            self.file
                .write_all(&init)
                .map_err(|e| AppError::Io(e.to_string()))?;
            self.started = true;
        }
        let fragment = fragment_bytes(self.sequence, &timed, &audio_timed)?;
        self.sequence += 1;
        self.file
            .write_all(&fragment)
            .map_err(|e| AppError::Io(e.to_string()))?;
        self.file.flush().map_err(|e| AppError::Io(e.to_string()))?;
        let bytes = self
            .file
            .metadata()
            .map_err(|e| AppError::Io(e.to_string()))?
            .len();
        Ok(bytes)
    }
}

/// 编码器输出的一帧：显示时间、Annex-B、是否同步样本。
pub type AnnexbFrame = (f64, Vec<u8>, bool);

/// 从已编码帧前面取 `count` 帧。`require_sync` 时第一帧必须是同步样本。
pub fn take_run(
    units: &mut Vec<AnnexbFrame>,
    count: usize,
    require_sync: bool,
) -> Option<(Vec<AnnexbFrame>, Option<f64>)> {
    if count == 0 || units.len() < count {
        return None;
    }
    if require_sync && !units[0].2 {
        return None;
    }
    let next = units.get(count).map(|unit| unit.0);
    Some((units.drain(0..count).collect(), next))
}

/// 第二个关键帧出现时，把它之前的样本切成一个封闭 GOP，并返回后一个关键帧的显示时间。
pub fn split_completed_gop(units: &mut Vec<AnnexbFrame>) -> Option<(Vec<AnnexbFrame>, f64)> {
    let mut keys = units
        .iter()
        .enumerate()
        .filter(|(_, unit)| unit.2)
        .map(|(index, _)| index);
    let first = keys.next()?;
    if first != 0 {
        return None;
    }
    let second = keys.next()?;
    let next_pts = units[second].0;
    Some((units.drain(0..second).collect(), next_pts))
}

/// 文件里已经有至少一个视频同步样本。经典 MP4（没有 `moof`）返回 false。
pub fn prefix_is_independently_decodable(path: &Path) -> bool {
    read_fragment_video(path)
        .ok()
        .is_some_and(|video| video.first_is_sync && !video.pts.is_empty())
}

/// 按片段顺序读出视频显示时间。损坏的箱返回错误。
pub fn read_fragment_video(path: &Path) -> AppResult<FragmentVideo> {
    let mut file = File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let len = file
        .metadata()
        .map_err(|e| AppError::Io(e.to_string()))?
        .len();
    let mut pos = 0u64;
    let mut pts = Vec::new();
    let mut first_is_sync = false;
    let mut saw_sample = false;
    let mut fragment_bases = Vec::new();
    while pos + 8 <= len {
        file.seek(SeekFrom::Start(pos))
            .map_err(|e| AppError::Io(e.to_string()))?;
        let mut header = [0u8; 16];
        file.read_exact(&mut header[..8])
            .map_err(|e| AppError::Io(e.to_string()))?;
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let (box_size, header_len) = match size32 {
            1 => {
                file.read_exact(&mut header[8..16])
                    .map_err(|e| AppError::Io(e.to_string()))?;
                (
                    u64::from_be_bytes(header[8..16].try_into().expect("8 字节")),
                    16u64,
                )
            }
            0 => (len - pos, 8u64),
            n => (u64::from(n), 8u64),
        };
        if box_size < header_len || pos + box_size > len {
            return Err(AppError::Media("片段箱长度越界".into()));
        }
        if &header[4..8] == b"moof" {
            let mut payload = vec![0u8; (box_size - header_len) as usize];
            file.read_exact(&mut payload)
                .map_err(|e| AppError::Io(e.to_string()))?;
            let (base, sync) = collect_video(&payload, &mut pts)?;
            if let Some(base) = base {
                fragment_bases.push(base);
            }
            if !saw_sample {
                first_is_sync = sync;
                saw_sample = !pts.is_empty();
            }
        }
        pos += box_size;
    }
    Ok(FragmentVideo {
        pts,
        first_is_sync,
        fragment_bases,
    })
}

fn collect_video(payload: &[u8], pts: &mut Vec<f64>) -> AppResult<(Option<u64>, bool)> {
    let mut base = None;
    let mut first_sync = false;
    let mut saw = false;
    for traf in parse_boxes(payload, 0, payload.len())
        .into_iter()
        .filter(|b| b.is("traf"))
    {
        let body = &payload[traf.start..traf.start + traf.size];
        let children = parse_boxes(body, 0, body.len());
        let Some(tfhd) = children.iter().find(|b| b.is("tfhd")) else {
            continue;
        };
        let tfhd_body = &body[tfhd.start..tfhd.start + tfhd.size];
        if tfhd_body.len() < 8 {
            return Err(AppError::Media("tfhd 过短".into()));
        }
        let track_id = u32::from_be_bytes(tfhd_body[4..8].try_into().expect("4 字节"));
        if track_id != VIDEO_TRACK {
            continue;
        }
        let Some(tfdt) = children.iter().find(|b| b.is("tfdt")) else {
            return Err(AppError::Media("视频片段缺少 tfdt".into()));
        };
        let tfdt_body = &body[tfdt.start..tfdt.start + tfdt.size];
        let dts_base = read_tfdt(tfdt_body)?;
        let Some(trun) = children.iter().find(|b| b.is("trun")) else {
            return Err(AppError::Media("视频片段缺少 trun".into()));
        };
        let trun_body = &body[trun.start..trun.start + trun.size];
        let sync = append_trun(trun_body, dts_base, pts)?;
        if !saw {
            first_sync = sync;
            saw = true;
        }
        base = Some(dts_base);
    }
    Ok((base, first_sync))
}

fn read_tfdt(payload: &[u8]) -> AppResult<u64> {
    if payload.is_empty() {
        return Err(AppError::Media("tfdt 过短".into()));
    }
    if payload[0] == 1 {
        if payload.len() < 12 {
            return Err(AppError::Media("tfdt 过短".into()));
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&payload[4..12]);
        return Ok(u64::from_be_bytes(buf));
    }
    if payload.len() < 8 {
        return Err(AppError::Media("tfdt 过短".into()));
    }
    Ok(u32::from_be_bytes(payload[4..8].try_into().expect("4 字节")) as u64)
}

fn append_trun(payload: &[u8], base: u64, pts: &mut Vec<f64>) -> AppResult<bool> {
    if payload.len() < 8 {
        return Err(AppError::Media("trun 过短".into()));
    }
    let flags = u32::from_be_bytes([0, payload[1], payload[2], payload[3]]);
    let count = u32::from_be_bytes(payload[4..8].try_into().expect("4 字节")) as usize;
    let mut off = 8usize;
    if flags & 0x1 != 0 {
        off += 4;
    }
    if flags & 0x4 != 0 {
        off += 4;
    }
    let has_dur = flags & 0x100 != 0;
    let has_size = flags & 0x200 != 0;
    let has_flags = flags & 0x400 != 0;
    let has_cto = flags & 0x800 != 0;
    let mut dts = base;
    let mut first_sync = false;
    for index in 0..count {
        let dur = if has_dur {
            let v = read_u32(payload, off)?;
            off += 4;
            v
        } else {
            0
        };
        if has_size {
            off += 4;
        }
        let sample_flags = if has_flags {
            let v = read_u32(payload, off)?;
            off += 4;
            v
        } else {
            0
        };
        let cto = if has_cto {
            let v = read_i32(payload, off)?;
            off += 4;
            v
        } else {
            0
        };
        if index == 0 {
            first_sync = sample_flags & NON_SYNC_BIT == 0;
        }
        let stamp = dts as i64 + i64::from(cto);
        pts.push(stamp as f64 / f64::from(MEDIA_TIMESCALE));
        dts += u64::from(dur);
    }
    Ok(first_sync)
}

fn read_u32(data: &[u8], at: usize) -> AppResult<u32> {
    let end = at + 4;
    let bytes: [u8; 4] = data
        .get(at..end)
        .ok_or_else(|| AppError::Media("trun 字段越界".into()))?
        .try_into()
        .expect("4 字节");
    Ok(u32::from_be_bytes(bytes))
}

fn read_i32(data: &[u8], at: usize) -> AppResult<i32> {
    Ok(read_u32(data, at)? as i32)
}

struct TimedSample {
    dts: u64,
    dur: u32,
    cto: i32,
    data: Vec<u8>,
    sync: bool,
}

fn trex_box(track_id: u32) -> Vec<u8> {
    let mut payload = 0u32.to_be_bytes().to_vec();
    payload.extend(track_id.to_be_bytes());
    payload.extend(1u32.to_be_bytes());
    payload.extend(0u32.to_be_bytes());
    payload.extend(0u32.to_be_bytes());
    payload.extend(0u32.to_be_bytes());
    build_box(b"trex", &payload)
}

fn time_samples(frames: &[(f64, Vec<u8>, bool)], as_avcc: bool) -> AppResult<Vec<TimedSample>> {
    if frames.is_empty() {
        return Ok(Vec::new());
    }
    let pts = frames
        .iter()
        .map(|(secs, _, _)| ticks(*secs))
        .collect::<AppResult<Vec<_>>>()?;
    let dts = decode_times(&pts);
    let durations = durations_of(&dts);
    frames
        .iter()
        .enumerate()
        .map(|(index, (_, data, sync))| {
            let cto = i32::try_from(pts[index] as i64 - dts[index] as i64)
                .map_err(|_| AppError::Media("合成偏移超出范围".into()))?;
            Ok(TimedSample {
                dts: dts[index],
                dur: durations[index],
                cto,
                data: if as_avcc {
                    avcc::to_avcc(data)
                } else {
                    data.clone()
                },
                sync: *sync,
            })
        })
        .collect()
}

fn ticks(secs: f64) -> AppResult<u64> {
    if !secs.is_finite() || secs < 0.0 {
        return Err(AppError::Media(format!("时间戳不能进入片段: {secs}")));
    }
    Ok((secs * f64::from(MEDIA_TIMESCALE)).round() as u64)
}

fn decode_times(pts: &[u64]) -> Vec<u64> {
    if pts.windows(2).all(|pair| pair[1] >= pair[0]) {
        return pts.to_vec();
    }
    let mut sorted = pts.to_vec();
    sorted.sort_unstable();
    let mut out = Vec::with_capacity(sorted.len());
    let mut prev = 0u64;
    for (index, value) in sorted.into_iter().enumerate() {
        let value = if index > 0 && value <= prev {
            prev + 1
        } else {
            value
        };
        out.push(value);
        prev = value;
    }
    out
}

fn durations_of(dts: &[u64]) -> Vec<u32> {
    let mut out: Vec<u32> = dts
        .windows(2)
        .map(|pair| u32::try_from(pair[1].saturating_sub(pair[0]).max(1)).unwrap_or(u32::MAX))
        .collect();
    let last = out.last().copied().unwrap_or(1);
    out.push(last);
    out
}

fn init_segment(
    width: usize,
    height: usize,
    audio: Option<&AudioHeader>,
    first_video: &[u8],
    timeline_secs: f64,
) -> AppResult<Vec<u8>> {
    let config =
        avcc::extract(first_video).ok_or_else(|| AppError::Media("首段没有 SPS/PPS".into()))?;
    let movie_ticks = (timeline_secs * f64::from(MOVIE_TIMESCALE)).round() as u64;
    let media_ticks = (timeline_secs * f64::from(MEDIA_TIMESCALE)).round() as u64;
    let mut moov = mvhd(movie_ticks, if audio.is_some() { 3 } else { 2 });
    moov.extend(video_trak(
        width as u32,
        height as u32,
        media_ticks,
        movie_ticks,
        &config,
    ));
    if let Some(audio) = audio {
        moov.extend(audio_trak(audio, media_ticks, movie_ticks));
    }
    let mut trex = trex_box(VIDEO_TRACK);
    if audio.is_some() {
        trex.extend(trex_box(AUDIO_TRACK));
    }
    moov.extend(build_box(b"mvex", &trex));
    let mut out = ftyp();
    out.extend(build_box(b"moov", &moov));
    Ok(out)
}

fn ftyp() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(b"iso5");
    payload.extend(0u32.to_be_bytes());
    payload.extend_from_slice(b"iso5");
    payload.extend_from_slice(b"isom");
    payload.extend_from_slice(b"iso6");
    payload.extend_from_slice(b"mp41");
    build_box(b"ftyp", &payload)
}

fn video_trak(
    width: u32,
    height: u32,
    media_ticks: u64,
    movie_ticks: u64,
    config: &avcc::AvcConfig,
) -> Vec<u8> {
    trak(TrakParts {
        track_id: VIDEO_TRACK,
        movie_ticks,
        media_ticks,
        width,
        height,
        volume: 0,
        handler: hdlr(b"vide", "VideoHandler"),
        header: vmhd(),
        stsd: video_stsd(width, height, config),
    })
}

fn audio_trak(audio: &AudioHeader, media_ticks: u64, movie_ticks: u64) -> Vec<u8> {
    trak(TrakParts {
        track_id: AUDIO_TRACK,
        movie_ticks,
        media_ticks,
        width: 0,
        height: 0,
        volume: 0x0100,
        handler: hdlr(b"soun", "SoundHandler"),
        header: smhd(),
        stsd: audio_stsd(audio.channels, audio.sample_rate, audio.esds.as_deref()),
    })
}

struct TrakParts {
    track_id: u32,
    movie_ticks: u64,
    media_ticks: u64,
    width: u32,
    height: u32,
    volume: u16,
    handler: Vec<u8>,
    header: Vec<u8>,
    stsd: Vec<u8>,
}

fn trak(parts: TrakParts) -> Vec<u8> {
    let mut mdia = mdhd(MEDIA_TIMESCALE, parts.media_ticks);
    mdia.extend(parts.handler);
    let mut minf = parts.header;
    minf.extend(dinf());
    minf.extend(empty_stbl(parts.stsd));
    mdia.extend(build_box(b"minf", &minf));
    let mut payload = tkhd(
        parts.track_id,
        parts.movie_ticks,
        parts.width,
        parts.height,
        parts.volume,
    );
    payload.extend(build_box(b"mdia", &mdia));
    build_box(b"trak", &payload)
}

fn empty_stbl(stsd: Vec<u8>) -> Vec<u8> {
    let version = 0u32.to_be_bytes();
    let mut stts = version.to_vec();
    stts.extend(0u32.to_be_bytes());
    let mut stsc = version.to_vec();
    stsc.extend(0u32.to_be_bytes());
    let mut stsz = version.to_vec();
    stsz.extend(0u32.to_be_bytes());
    stsz.extend(0u32.to_be_bytes());
    let mut stco = version.to_vec();
    stco.extend(0u32.to_be_bytes());
    let mut payload = stsd;
    payload.extend(build_box(b"stts", &stts));
    payload.extend(build_box(b"stsc", &stsc));
    payload.extend(build_box(b"stsz", &stsz));
    payload.extend(build_box(b"stco", &stco));
    build_box(b"stbl", &payload)
}

fn fragment_bytes(
    sequence: u32,
    video: &[TimedSample],
    audio: &[TimedSample],
) -> AppResult<Vec<u8>> {
    let video_payload: usize = video.iter().map(|s| s.data.len()).sum();
    let audio_payload: usize = audio.iter().map(|s| s.data.len()).sum();
    let video_trun = trun_box(video, 0);
    let audio_trun = (!audio.is_empty()).then(|| trun_box(audio, 0));
    let video_traf = traf_box(VIDEO_TRACK, video[0].dts, &video_trun);
    let audio_traf = audio_trun
        .as_ref()
        .map(|trun| traf_box(AUDIO_TRACK, audio[0].dts, trun));
    let mut payload = mfhd(sequence);
    let video_patch = payload.len() + data_offset_index(&video_traf);
    payload.extend(video_traf);
    let audio_patch = audio_traf
        .as_ref()
        .map(|traf| payload.len() + data_offset_index(traf));
    if let Some(traf) = audio_traf {
        payload.extend(traf);
    }
    let mut moof = build_box(b"moof", &payload);
    let mdat_payload = video_payload + audio_payload;
    let video_offset = moof.len() as u32 + 8;
    patch_u32(&mut moof, 8 + video_patch, video_offset);
    if let Some(at) = audio_patch {
        patch_u32(&mut moof, 8 + at, video_offset + video_payload as u32);
    }
    let mut out = moof;
    let mdat_size = (mdat_payload as u32)
        .checked_add(8)
        .ok_or_else(|| AppError::Media("片段 mdat 超过 4GB".into()))?;
    out.extend(mdat_size.to_be_bytes());
    out.extend_from_slice(b"mdat");
    for sample in video {
        out.extend_from_slice(&sample.data);
    }
    for sample in audio {
        out.extend_from_slice(&sample.data);
    }
    Ok(out)
}

fn data_offset_index(traf: &[u8]) -> usize {
    let children = parse_boxes(traf, 8, traf.len());
    let trun = children
        .iter()
        .find(|b| b.is("trun"))
        .expect("traf 里有 trun");
    trun.start + 8
}

fn patch_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

fn mfhd(sequence: u32) -> Vec<u8> {
    let mut payload = 0u32.to_be_bytes().to_vec();
    payload.extend(sequence.to_be_bytes());
    build_box(b"mfhd", &payload)
}

fn traf_box(track_id: u32, base: u64, trun: &[u8]) -> Vec<u8> {
    let mut payload = tfhd(track_id);
    payload.extend(tfdt(base));
    payload.extend_from_slice(trun);
    build_box(b"traf", &payload)
}

fn tfhd(track_id: u32) -> Vec<u8> {
    let mut payload = TFHD_DEFAULT_BASE_IS_MOOF.to_be_bytes().to_vec();
    payload.extend(track_id.to_be_bytes());
    build_box(b"tfhd", &payload)
}

fn tfdt(base: u64) -> Vec<u8> {
    let mut payload = Vec::new();
    if base > u64::from(u32::MAX) {
        payload.push(1);
        payload.extend([0u8; 3]);
        payload.extend(base.to_be_bytes());
    } else {
        payload.extend(0u32.to_be_bytes());
        payload.extend((base as u32).to_be_bytes());
    }
    build_box(b"tfdt", &payload)
}

fn trun_box(samples: &[TimedSample], data_offset: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(1);
    payload.extend_from_slice(&TRUN_FLAGS.to_be_bytes()[1..4]);
    payload.extend((samples.len() as u32).to_be_bytes());
    payload.extend(data_offset.to_be_bytes());
    for sample in samples {
        payload.extend(sample.dur.to_be_bytes());
        payload.extend((sample.data.len() as u32).to_be_bytes());
        let flags = if sample.sync {
            SAMPLE_SYNC
        } else {
            SAMPLE_NON_SYNC
        };
        payload.extend(flags.to_be_bytes());
        payload.extend(sample.cto.to_be_bytes());
    }
    build_box(b"trun", &payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f64 = 1.0 / 30.0;

    fn annexb(nals: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nals {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(nal);
        }
        out
    }

    fn idr(at: f64) -> (f64, Vec<u8>, bool) {
        (
            at,
            annexb(&[&[0x67, 0x42, 0x00, 0x1f], &[0x68, 0xce], &[0x65, 0x88]]),
            true,
        )
    }

    fn inter(at: f64) -> (f64, Vec<u8>, bool) {
        (at, annexb(&[&[0x41, 0x9a, 0x11]]), false)
    }

    #[test]
    fn a_second_keyframe_closes_the_first_gop() {
        let mut units = vec![idr(0.0), inter(STEP), idr(STEP * 2.0)];
        let (gop, next) = split_completed_gop(&mut units).expect("应切出封闭 GOP");
        assert_eq!(gop.len(), 2);
        assert!((next - STEP * 2.0).abs() < 1e-9);
        assert_eq!(units.len(), 1);
        assert!(split_completed_gop(&mut units).is_none());
    }

    #[test]
    fn an_open_gop_stays_buffered() {
        let mut units = vec![idr(0.0), inter(STEP)];
        assert!(split_completed_gop(&mut units).is_none());
        assert_eq!(units.len(), 2);
    }

    #[test]
    fn the_first_gop_is_playable_and_a_later_gop_appends() {
        let dir = std::env::temp_dir().join(format!("hg-frag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grow.mp4");
        let audio = AudioTrack {
            sample_rate: 44100,
            channels: 2,
            esds: None,
            samples: vec![(0.0, vec![0x11, 0x22]), (STEP * 2.0, vec![0x33])],
        };
        let mut growing = GrowingH264::create(&path, 64, 64, Some(&audio), 1.0).unwrap();
        let bytes = growing
            .push(&[idr(0.0), inter(STEP)], &[audio.samples[0].clone()])
            .unwrap();
        let first = read_fragment_video(&path).unwrap();
        assert!(prefix_is_independently_decodable(&path));
        assert_eq!(first.pts.len(), 2);
        assert!(first.first_is_sync);
        assert_eq!(first.fragment_bases.len(), 1);
        assert!(bytes > 0);
        let again = growing
            .push(
                &[idr(STEP * 2.0), inter(STEP * 3.0)],
                &[audio.samples[1].clone()],
            )
            .unwrap();
        assert!(again > bytes);
        let both = read_fragment_video(&path).unwrap();
        assert_eq!(both.pts.len(), 4);
        assert_eq!(both.fragment_bases.len(), 2);
        assert!(both.fragment_bases[1] > both.fragment_bases[0]);
        assert!((both.pts[0]).abs() < 1e-6);
        assert!((both.pts[2] - STEP * 2.0).abs() < 1e-4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_is_not_an_independent_prefix() {
        let dir = std::env::temp_dir().join(format!("hg-frag-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("no.mp4");
        std::fs::write(&path, b"not a fragment").unwrap();
        assert!(!prefix_is_independently_decodable(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
