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
    /// 帧率，用于码率控制与时间戳换算。
    ///
    /// 只是**兜底值**：源样本表能推出平均帧率时（绝大多数正常片源）一律用
    /// 真实值，逐帧时间戳也来自源时间轴；这个默认值只在容器没有 `stts` 时生效。
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
    /// 实际走的后端（并行度/超订信号以它为准，别再猜字符串）
    pub backend: crate::media::Backend,
}

/// 一条音轨的采集结果：封装参数 + 逐帧 `(时间戳, ADTS 帧)`。
type AudioTrack = (audio::AudioFormat, Vec<(f64, Vec<u8>)>);

/// 解码出的 YUV 帧：`(y, u, v, stride_y, stride_c)`。
pub(super) type YuvFrame = (Vec<u8>, Vec<u8>, Vec<u8>, usize, usize);

/// 源时间轴：逐样本显示时间与平均帧率。
///
/// 从样本表的 `stts`/`ctts` 展开（见 `domain::mp4::timing`）。空 `pts` 表示
/// 容器没给时间轴，调用方回落到「样本号 / 默认帧率」的合成轴——那是
/// 没有时间信息的畸形文件才走的路，正常片源一律用真实时间戳。
pub(super) struct SourceTiming {
    /// 每样本显示时间（秒），与样本表等长；空 = 无时间轴信息
    pub pts: Vec<f64>,
    /// 平均帧率，用于编码器码控提示与封装层声明
    pub framerate: f32,
}

impl SourceTiming {
    fn of(info: &crate::domain::mp4::sample_table::TrackInfo, fallback_framerate: f32) -> Self {
        SourceTiming {
            pts: info.sample_pts(),
            framerate: info
                .average_framerate()
                .map(|f| f as f32)
                .unwrap_or(fallback_framerate),
        }
    }
}

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

    // 源时间轴。固定 25fps 的合成时间轴曾是默认：源不是 25fps 时视频轨整体
    // 变速、与按真实时间累计的音轨漂移。现在只有拿不到 stts 的畸形文件才回落。
    let timing = SourceTiming::of(&video.info, options.framerate);

    // 2) HEVC 解码 → H.264 编码（流水线，见 `codec`）
    //
    // `rusty_h265-accel` 的去块滤波里有一处越界（`deblock.rs` 的 `ok` 判定
    // 覆盖不到某些边界段）。**release 构建不受影响**：它只是 `debug_assert!`，
    // 且 SIMD 路径由 `if ok` 守住，会整段跳过。实测 release 下 1080p 正常解码，
    // 帧数与样本数一致。
    //
    // 仍要包 `catch_unwind`：debug 构建会在这条断言上 panic，而直接崩掉
    // 整个应用比报错糟糕得多。release 下这层是纯保险。
    let encoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        codec::decode_and_encode(
            input,
            &annexb_prefix,
            length_size,
            &video.info.samples,
            options,
            &timing,
        )
    }))
    .map_err(|_| {
        AppError::Media(
            "纯 Rust 软解在该码流上崩溃（debug 构建的 rusty_h265 断言），请改用 release 构建或安装 ffmpeg"
                .into(),
        )
    })??;

    // 3) 音轨：解密后已是明文 AAC，按时间戳直接带过去
    let audio = collect_audio(input, &demuxed)?;

    // 4) 封装成 MP4。封装层声明的帧率用源的实际平均帧率，
    //    与逐帧真实 PTS 一致；拿不到时间轴时与 options 默认值相同。
    let mut mux_options = options.clone();
    mux_options.framerate = timing.framerate;
    let size = mux::mux_h264(
        output,
        &encoded.units,
        encoded.width,
        encoded.height,
        audio.as_ref().map(|(f, s)| (*f, s.as_slice())),
        &mux_options,
    )?;

    Ok(TranscodeResult {
        output_path: output.to_string_lossy().to_string(),
        output_size: size,
        elapsed_ms: started.elapsed().as_millis(),
        frames: encoded.frames as u64,
        decoder: "rusty_h265".into(),
        encoder: "rusty_h264".into(),
        backend: crate::media::Backend::Rust,
    })
}

/// 平台后端复用的「音轨直通 + 封装」入口。
///
/// 与软解路径共享同一套 [`collect_audio`] 与 [`mux::mux_h264`]——音轨不解码、
/// AAC 直通、faststart 封装，任何后端产出的 MP4 完全同构。
pub(super) fn mux_with_audio(
    input: &Path,
    demuxed: &crate::media::demux::Demuxed,
    output: &Path,
    units: &[(f64, Vec<u8>, bool)],
    width: usize,
    height: usize,
    framerate: f32,
) -> AppResult<u64> {
    let audio = collect_audio(input, demuxed)?;
    let options = TranscodeOptions {
        framerate,
        ..Default::default()
    };
    mux::mux_h264(
        output,
        units,
        width,
        height,
        audio.as_ref().map(|(f, s)| (*f, s.as_slice())),
        &options,
    )
}

/// 从文件读一段字节（解码样本用）。
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

    // 只读音轨样本所在的区间，不把整个文件读进来：
    // 视频轨动辄几十上百 MB，整文件读一次会白占一份与转码无关的内存。
    // 音轨样本按 offset 升序切段，顺序读文件正好顺带利用预读。
    let format = audio::read_audio_format(input, &track.info)?;

    // AAC 每帧固定 1024 个采样，时间戳按累计秒数给
    let mut pts = 0.0f64;
    let mut samples = Vec::with_capacity(track.info.samples.len());
    let mut file = std::fs::File::open(input).map_err(|e| AppError::Io(e.to_string()))?;
    for &(offset, size) in &track.info.samples {
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| AppError::Io(e.to_string()))?;
        let mut frame = vec![0u8; size as usize];
        file.read_exact(&mut frame)
            .map_err(|e| AppError::Media(format!("读取音轨样本失败: {e}")))?;
        // MP4 里是裸 AAC 帧，muxide 要 ADTS framing，逐帧补头
        samples.push((
            pts,
            audio::adts_frame(&frame, format.sample_rate, format.channels),
        ));
        pts += 1024.0 / f64::from(format.sample_rate.max(1));
    }

    Ok(Some((format, samples)))
}
