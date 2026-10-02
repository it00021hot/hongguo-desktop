//! HEVC → H.264 转码：解码 → 编码 → 封装。
//!
//! 数据流：
//!
//! ```text
//! MP4 in ─demux→ HEVC Annex-B ─rusty_h265→ YUV 4:2:0
//!                                            │
//!                                     rusty_h264 → H.264 Annex-B
//!                                            │
//!                                        muxide → MP4 out
//! ```
//!
//! 与 [`super::remux`] 只差「是否重编码」——按「一个问题一个解决方案」的
//! 原则，合并与转码共用一条流水线，用参数区分，不写两套解析逻辑。

use std::path::Path;

use crate::error::{AppError, AppResult};

mod audio;
mod codec;
mod mux;

/// 转码参数（对齐现版 ffmpeg 参数）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TranscodeOptions {
    /// 恒定质量，对应现版 `-crf 23`。编码器以 QP 表达，取值 0–51。
    pub qp: u8,
    /// 关键帧间隔，对应 x264 默认 250
    pub gop_size: u32,
    /// 帧率，用于码率控制与时间戳换算
    pub framerate: f32,
}

impl Default for TranscodeOptions {
    fn default() -> Self {
        Self {
            qp: 23,
            gop_size: 250,
            framerate: 25.0,
        }
    }
}

impl TranscodeOptions {
    /// 收敛 QP 到编码器接受的范围。
    fn clamped_qp(&self) -> u8 {
        self.qp.min(51)
    }
}

/// 转码结果。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TranscodeResult {
    pub output_path: String,
    pub output_size: u64,
    pub elapsed_ms: u128,
    /// 实际处理的帧数
    pub frames: u64,
    /// 实际使用的解码后端
    pub decoder: String,
    pub encoder: String,
}

/// 一条音轨的采集结果：封装参数 + 逐帧 `(时间戳, ADTS 帧)`。
type AudioTrack = (audio::AudioFormat, Vec<(f64, Vec<u8>)>);

/// 解码出的 YUV 帧：`(y, u, v, stride_y, stride_c)`。
pub(super) type YuvFrame = (Vec<u8>, Vec<u8>, Vec<u8>, usize, usize);

/// 执行转码：输入已解密的 MP4，输出可播放的 H.264 MP4。
pub fn transcode_file(
    input: &Path,
    output: &Path,
    options: &TranscodeOptions,
) -> AppResult<TranscodeResult> {
    let started = std::time::Instant::now();

    // 1) 解复用，拿到样本表
    let demuxed = crate::media::demux::demux_file(input)?;
    let video = demuxed
        .video_track()
        .ok_or_else(|| AppError::Media("没有视频轨".into()))?;

    if video.info.samples.is_empty() {
        return Err(AppError::Media("视频轨没有样本".into()));
    }

    // hvcC 参数集（VPS/SPS/PPS）必须先喂给解码器，否则首个 IRAP 无法解。
    // length_size 决定样本的长度前缀宽度，转 Annex-B 时要用。
    let (annexb_prefix, length_size) = crate::media::hevc::read_parameter_sets(input, &video.info)?;

    // 2) HEVC 解码 → YUV
    //
    // `rusty_h265-accel` 的去块滤波里有一处越界（`deblock.rs` 的 `ok` 判定
    // 覆盖不到某些边界段）。**release 构建不受影响**：它只是 `debug_assert!`，
    // 且 SIMD 路径由 `if ok` 守住，会整段跳过。实测 release 下 1080p 正常解码，
    // 帧数与样本数一致。
    //
    // 仍要包 `catch_unwind`：debug 构建会在这条断言上 panic，而直接崩掉
    // 整个应用比报错糟糕得多。release 下这层是纯保险。
    let decode = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        codec::decode_hevc(input, &annexb_prefix, length_size, &video.info.samples)
    }))
    .map_err(|_| {
        AppError::Media(
            "纯 Rust 软解在该码流上崩溃（debug 构建的 rusty_h265 断言），请改用 release 构建或安装 ffmpeg"
                .into(),
        )
    })??;

    let (width, height, yuv_frames) = decode;

    if yuv_frames.is_empty() {
        return Err(AppError::Media("HEVC 解码没有产出任何帧".into()));
    }

    // 3) H.264 编码
    let encoded = codec::encode_h264(&yuv_frames, width, height, options)?;

    // 4) 音轨：解密后已是明文 AAC，按时间戳直接带过去
    let audio = collect_audio(input, &demuxed)?;

    // 5) 封装成 MP4
    let size = mux::mux_h264(
        output,
        &encoded,
        width,
        height,
        audio.as_ref().map(|(f, s)| (*f, s.as_slice())),
        options,
    )?;

    Ok(TranscodeResult {
        output_path: output.to_string_lossy().to_string(),
        output_size: size,
        elapsed_ms: started.elapsed().as_millis(),
        frames: yuv_frames.len() as u64,
        decoder: "rusty_h265".into(),
        encoder: "rusty_h264".into(),
    })
}

/// HEVC 解码为 YUV 帧序列。
pub(super) fn read_range(path: &Path, offset: u64, size: u64) -> AppResult<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    f.seek(SeekFrom::Start(offset))
        .map_err(|e| AppError::Io(e.to_string()))?;
    let mut buf = vec![0u8; size as usize];
    f.read_exact(&mut buf)
        .map_err(|e| AppError::Media(format!("读取样本失败: {e}")))?;
    Ok(buf)
}

#[cfg(test)]
#[path = "transcode_tests.rs"]
mod tests;

/// 收集音轨：AAC 帧 + 采样率/声道。
///
/// 源音轨在解密后已是明文 AAC，**原样搬运**即可——不解码成 PCM，也不重新编码。
/// 没有音轨时返回 `None`，产物就是纯视频（与 ffmpeg `-an` 的行为一致）。
fn collect_audio(
    input: &Path,
    demuxed: &crate::media::demux::Demuxed,
) -> AppResult<Option<AudioTrack>> {
    let Some(track) = demuxed.audio_track() else {
        return Ok(None);
    };
    if track.info.samples.is_empty() {
        return Ok(None);
    }

    let data = std::fs::read(input).map_err(|e| AppError::Io(e.to_string()))?;
    let format = audio::read_audio_format(&data, &track.info)?;

    // AAC 每帧固定 1024 个采样，时间戳按累计秒数给
    let mut pts = 0.0f64;
    let mut samples = Vec::with_capacity(track.info.samples.len());
    for &(offset, size) in &track.info.samples {
        let start = offset as usize;
        let end = start + size as usize;
        let frame = data
            .get(start..end)
            .ok_or_else(|| AppError::Media("音轨样本越界".into()))?;
        // MP4 里是裸 AAC 帧，muxide 要 ADTS framing，逐帧补头
        samples.push((
            pts,
            audio::adts_frame(frame, format.sample_rate, format.channels),
        ));
        pts += 1024.0 / f64::from(format.sample_rate.max(1));
    }

    Ok(Some((format, samples)))
}
