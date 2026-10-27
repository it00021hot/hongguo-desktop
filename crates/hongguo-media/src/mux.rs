//! H.264 → MP4 封装。
//!
//! 平台编码器（VideoToolbox / Media Foundation）吐出的 H.264 是 Annex-B 形态的
//! 裸码流，这里把它封成 MP4：音视频交织写进 `mdat`，样本表与参数集写进 `moov`。
//!
//! 四条关键决定：
//!
//! - **faststart 恒开**（`moov` 在 `mdat` 之前）：产物走自定义协议送进播放器，
//!   `moov` 在前才能在只读到文件头时就拿到时长与样本表。偏移在写盘前就算定，
//!   不像「先写媒体数据再回头搬 `moov`」那样需要按写入序重排样本边界。
//! - **B 帧按解码序存放，`ctts` 记录合成偏移**：平台硬编器关不掉帧重排
//!   （本机 VT 会话对 `MaxFrameDelayCount=0` 与 `AllowFrameReordering=0` 都拒收），
//!   码流天然是解码序 + 非单调显示时间。解码时间取「第 k 小的显示时间」，
//!   合成偏移 = 显示时间 − 解码时间（有符号，`ctts` version 1）。
//! - **每样本一个 chunk**：`stco` 直接给每个样本的绝对偏移，不必再按 `stsc`
//!   展开 chunk 映射。
//! - **音轨直通**：源音轨解密后已是明文 AAC，原样搬进产物，不解码成 PCM 再重编码。
//!
//! 字节组装层（`moov` / `mdat` 布局）在 [`crate::mp4`]——快速合并与重新封装
//! 共用同一份。

mod audio;
mod avcc;
mod track;

use std::io::Write;
use std::path::Path;

use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::build_box;

use crate::mp4::{TrackSpec, build_moov, mdat_data_start, mdat_header, needs_wide_offsets};

pub use audio::AudioTrack;
use track::{AudioSpec, VideoSpec};

/// 媒体时基：90kHz。视频与音频共用，25/30/60fps 都能整除。
pub const MEDIA_TIMESCALE: u32 = 90_000;
/// 电影时基：毫秒。
pub const MOVIE_TIMESCALE: u32 = 1000;

/// 音轨的封装计划：裁掉早于首帧视频的样本。
struct AudioPlan<'a> {
    /// 裁剪后的显示时间（媒体刻度）
    ticks: Vec<u64>,
    /// 裁剪后的样本数据
    frames: &'a [(f64, Vec<u8>)],
}

impl<'a> AudioPlan<'a> {
    /// 规划一条音轨。
    ///
    /// 源片音轨可以比视频轨先开始（红果片源实测：音频 0s、视频首帧 0.133s
    /// ——AAC priming + 视频轨延迟起步）。这段头部按 `elst` 语义该裁掉：
    /// 容器没有 edit list 时 presentation 时间轴从 0 起算，早于首帧的音频
    /// 会让播放器在画面出来之前先播一段没画面的声音。
    fn new(audio: &'a AudioTrack, first_video_tick: u64) -> AppResult<Self> {
        let all = to_ticks(audio.samples.iter().map(|(pts, _)| *pts))?;
        let start = all.partition_point(|t| *t < first_video_tick);
        if start >= audio.samples.len() {
            return Err(AppError::Media(
                "音轨整段都早于视频首帧，没有可封装的音频".into(),
            ));
        }
        Ok(Self {
            ticks: all[start..].to_vec(),
            frames: &audio.samples[start..],
        })
    }

    /// 音轨的索引素材。
    fn spec(&self, audio: &AudioTrack) -> AudioSpec {
        let stts = runs_u32(&deltas(&self.ticks));
        AudioSpec {
            track_id: 2,
            channels: audio.channels,
            sample_rate: audio.sample_rate,
            esds: audio.esds.clone(),
            sizes: self.frames.iter().map(|(_, d)| d.len() as u32).collect(),
            duration: total(&stts),
            stts,
        }
    }
}

/// 封装成 MP4（faststart）。
///
/// `frames` 是编码器的输出：`(显示时间秒, Annex-B 数据, 是否关键帧)`，
/// 顺序是**解码序**（带 B 帧的码流天然如此）。`audio` 传 `None` 表示没有音轨。
pub fn mux_h264(
    output: &Path,
    frames: &[(f64, Vec<u8>, bool)],
    width: usize,
    height: usize,
    audio: Option<&AudioTrack>,
) -> AppResult<u64> {
    if frames.is_empty() {
        return Err(AppError::Media("没有可封装的视频帧".into()));
    }
    if width > u16::MAX as usize || height > u16::MAX as usize {
        return Err(AppError::Media(format!(
            "分辨率 {width}x{height} 超出 MP4 容器上限"
        )));
    }
    // 首帧必须是关键帧：播放器靠它起播，产物没有同步起点就只会黑屏
    if !frames[0].2 {
        return Err(AppError::Media("第一帧不是关键帧，产物没有同步起点".into()));
    }
    let avc = avcc::extract(&frames[0].1)
        .ok_or_else(|| AppError::Media("视频流里没有 SPS/PPS，构造不出 avcC".into()))?;

    // 1) 视频样本：Annex-B → AVCC
    let mut video_data: Vec<Vec<u8>> = Vec::with_capacity(frames.len());
    for (i, (_, data, _)) in frames.iter().enumerate() {
        if data.is_empty() {
            return Err(AppError::Media(format!("第 {i} 帧是空的")));
        }
        video_data.push(avcc::to_avcc(data));
    }

    // 2) 视频时间轴
    let video_pts = to_ticks(frames.iter().map(|(pts, ..)| *pts))?;
    let video_dts = decode_ticks(&video_pts);
    let video_stts = runs_u32(&deltas(&video_dts));
    let cts_values = cts(&video_pts, &video_dts);
    // 全是 0 就说明没有 B 帧——`ctts` 这张表不必写
    let video_ctts = if cts_values.iter().all(|v| *v == 0) {
        Vec::new()
    } else {
        runs_i32(&cts_values)
    };

    // 3) 音轨：裁掉早于首帧视频的样本
    let audio_plan = match audio {
        Some(a) => Some(AudioPlan::new(a, video_pts[0])?),
        None => None,
    };

    // 4) 交织顺序：视频按解码时间、音频按显示时间，同一时刻视频在前。
    //
    // 视频样本在 `mdat` 里必须保持解码序——`stco` 按写入序记录，而
    // `stsz`/`stts`/`ctts` 按样本序构建，两者错位整条轨就解不出来。
    let mut schedule: Vec<(u64, u8, usize)> =
        Vec::with_capacity(video_data.len() + audio_plan.as_ref().map_or(0, |p| p.frames.len()));
    for (i, dts) in video_dts.iter().enumerate() {
        schedule.push((*dts, 0, i));
    }
    if let Some(plan) = &audio_plan {
        for (i, ticks) in plan.ticks.iter().enumerate() {
            schedule.push((*ticks, 1, i));
        }
    }
    schedule.sort_by_key(|(tick, kind, idx)| (*tick, *kind, *idx));

    // 5) 轨道素材
    let video_spec = VideoSpec {
        track_id: 1,
        width: width as u32,
        height: height as u32,
        avc,
        sizes: video_data.iter().map(|d| d.len() as u32).collect(),
        duration: total(&video_stts),
        stts: video_stts,
        ctts: video_ctts,
        stss: sync_samples(frames),
    };
    let mut specs = vec![video_spec.build()];
    if let (Some(plan), Some(audio)) = (&audio_plan, audio) {
        specs.push(plan.spec(audio).build());
    }
    let mvhd = track::mvhd(movie_duration(&specs), specs.len() as u32 + 1);

    // 6) 偏移：每样本在产物里的绝对位置
    let total_payload: u64 = specs
        .iter()
        .flat_map(|s| s.sizes.iter())
        .map(|v| u64::from(*v))
        .sum();
    let wide = needs_wide_offsets(total_payload);
    let ftyp = ftyp();
    let mut offsets: Vec<Vec<u64>> = specs.iter().map(|s| vec![0u64; s.sizes.len()]).collect();
    // moov 的长度只取决于样本数与偏移宽度，与偏移的具体数值无关：
    // 先用占位偏移量一次长度，再带真实偏移重算一次。
    let moov_len = build_moov(&mvhd, &specs, &offsets, wide).len();
    let data_start = mdat_data_start(ftyp.len(), moov_len, wide);
    let mut cursor = data_start;
    for (_, kind, idx) in &schedule {
        let track = *kind as usize;
        offsets[track][*idx] = cursor;
        cursor += u64::from(specs[track].sizes[*idx]);
    }
    let moov = build_moov(&mvhd, &specs, &offsets, wide);
    debug_assert_eq!(moov.len(), moov_len, "两次构建的 moov 长度应一致");

    // 7) 落盘：[ftyp][moov][mdat]
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let temp = hongguo_common::utils::path::temp_path_for(output);
    // 上一轮崩在中途会留下半个临时文件，先清掉再动手
    let _ = std::fs::remove_file(&temp);
    {
        let mut file = std::fs::File::create(&temp).map_err(|e| AppError::Io(e.to_string()))?;
        let write = |file: &mut std::fs::File, buf: &[u8]| -> AppResult<()> {
            file.write_all(buf).map_err(|e| AppError::Io(e.to_string()))
        };
        write(&mut file, &ftyp)?;
        write(&mut file, &moov)?;
        write(&mut file, &mdat_header(total_payload, wide))?;
        for (_, kind, idx) in &schedule {
            let data = match kind {
                0 => &video_data[*idx],
                _ => &audio_plan.as_ref().expect("音频计划存在").frames[*idx].1,
            };
            write(&mut file, data)?;
        }
        file.flush().map_err(|e| AppError::Io(e.to_string()))?;
    }
    let size = std::fs::metadata(&temp)
        .map_err(|e| AppError::Io(e.to_string()))?
        .len();
    std::fs::rename(&temp, output).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        AppError::Io(format!("原子替换失败: {e}"))
    })?;
    Ok(size)
}

/// 平台后端复用的「音轨直通 + 封装」入口。
///
/// 音轨不解码、AAC 直通、faststart 封装，任何后端产出的 MP4 完全同构。
///
/// 对外可见是刻意的：平台硬编（VT / MF）在 `hongguo-media-platform`，与本 crate
/// 不同 crate，但它必须产出与其他后端同构的容器。
pub fn with_audio(
    input: &Path,
    demuxed: &crate::demux::Demuxed,
    output: &Path,
    units: &[(f64, Vec<u8>, bool)],
    width: usize,
    height: usize,
) -> AppResult<u64> {
    let audio = audio::collect(input, demuxed)?;
    mux_h264(output, units, width, height, audio.as_ref())
}

/// `ftyp`：声明容器品牌。
fn ftyp() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"isom"); // major_brand
    p.extend(0x0000_0200u32.to_be_bytes()); // minor_version
    p.extend_from_slice(b"isom");
    p.extend_from_slice(b"mp41");
    build_box(b"ftyp", &p)
}

/// 秒 → 媒体刻度。
fn to_ticks(values: impl Iterator<Item = f64>) -> AppResult<Vec<u64>> {
    let mut out = Vec::new();
    for v in values {
        if !v.is_finite() {
            return Err(AppError::Media("时间戳不是有限数".into()));
        }
        if v < 0.0 {
            return Err(AppError::Media(format!("时间戳为负: {v}")));
        }
        out.push((v * f64::from(MEDIA_TIMESCALE)).round() as u64);
    }
    Ok(out)
}

/// 解码时间：第 k 个样本的解码槽位取「第 k 小的显示时间」。
///
/// 无 B 帧的流显示序与解码序一致，直接用显示时间。B 帧流必须重排——
/// `stts` 描述的是解码时间轴，`ctts` 记录合成偏移，两者合起来才是显示时间。
/// 重复的显示时间（畸形源）按一个刻度顶开：解码时间必须严格递增，
/// 否则解码器会认为时间轴倒退。
fn decode_ticks(pts: &[u64]) -> Vec<u64> {
    if pts.windows(2).all(|w| w[1] >= w[0]) {
        return pts.to_vec();
    }
    let mut sorted = pts.to_vec();
    sorted.sort_unstable();
    let mut out = Vec::with_capacity(sorted.len());
    let mut prev = 0u64;
    for (i, v) in sorted.into_iter().enumerate() {
        let v = if i > 0 && v <= prev { prev + 1 } else { v };
        out.push(v);
        prev = v;
    }
    out
}

/// 逐样本时长：相邻两点的差。最后一个沿用前一个（只有一个样本时为 1）。
fn deltas(ticks: &[u64]) -> Vec<u32> {
    if ticks.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<u32> = (1..ticks.len())
        .map(|k| {
            ticks[k]
                .saturating_sub(ticks[k - 1])
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .collect();
    let last = out.last().copied().unwrap_or(1);
    out.push(last);
    out
}

/// 合成偏移：显示时间 − 解码时间。
fn cts(pts: &[u64], dts: &[u64]) -> Vec<i32> {
    pts.iter()
        .zip(dts)
        .map(|(p, d)| offset_i32(*p as i64 - *d as i64))
        .collect()
}

/// 差值转成有符号 32 位（`ctts` 的取值范围）。
///
/// 超出范围时饱和而不是截断——截断出来的偏移会让播放器把帧放到错误的时间点。
fn offset_i32(diff: i64) -> i32 {
    i32::try_from(diff).unwrap_or(if diff < 0 { i32::MIN } else { i32::MAX })
}

/// 同步样本号（1 起）。
///
/// 全是关键帧时不写 `stss`——那张表缺省就是「全部同步」。
fn sync_samples(frames: &[(f64, Vec<u8>, bool)]) -> Option<Vec<u32>> {
    if frames.iter().all(|(_, _, key)| *key) {
        return None;
    }
    Some(
        frames
            .iter()
            .enumerate()
            .filter(|(_, (_, _, key))| *key)
            .map(|(i, _)| i as u32 + 1)
            .collect(),
    )
}

/// 游程压缩：相邻相同的值并成一条。
fn runs_u32(values: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &v in values {
        crate::mp4::push_run(&mut out, 1, v);
    }
    out
}

/// 游程压缩（合成偏移）。
fn runs_i32(values: &[i32]) -> Vec<(u32, i32)> {
    let mut out: Vec<(u32, i32)> = Vec::new();
    for &v in values {
        crate::mp4::push_run(&mut out, 1, v);
    }
    out
}

/// 一条轨的媒体时长 = 各游程的 `count × delta` 之和。
fn total(runs: &[(u32, u32)]) -> u64 {
    runs.iter().map(|&(n, d)| u64::from(n) * u64::from(d)).sum()
}

/// 电影时长：各轨 presentation 时长的最大值。
fn movie_duration(specs: &[TrackSpec]) -> u64 {
    specs
        .iter()
        .map(|s| track::to_movie(total(&s.stts)))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "mux/tests.rs"]
mod tests;
