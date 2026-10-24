//! 转码编排（与 vt 侧逐段对应）：HEVC→H.264 主流程（[`transcode_h264`]
//! 入口、[`run`] 流水）、逐帧喂编码的 [`process_frame`] / [`emit_outputs`]
//! 与时间换算小工具。

use std::time::Instant;

use hongguo_common::{AppError, AppResult};
use hongguo_video::sample_table::TrackInfo;

use hongguo_media_platform_api::PlatformRequest;
use hongguo_media_platform_api::video::bitrate_for;

use super::decoder::{Decoder, Nv12Frame};
use super::encoder::{Encoder, EncoderKind};
use super::mft::run_on_mf_thread;
use super::probe::h264_hw_encoder_available;
use super::scaler::Scaler;

/// 平台硬编可用时执行转码；不可用返回 `None`（HEVC 以外的源也返回 `None`，
/// 留给 ffmpeg——H.264 源在 ffmpeg 侧是直转，没必要绕 GPU）。
pub fn transcode_h264(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    if !h264_hw_encoder_available() {
        return None;
    }
    let demuxed = hongguo_media::demux::demux_file(req.input).ok()?;
    let video = demuxed.video_track()?;
    if !hongguo_media::hevc::is_hevc(&video.info.codec) {
        return None;
    }
    Some(run_on_mf_thread(|| {
        run(req, &demuxed, &video.info, EncoderKind::Hardware)
    }))
}

/// 仅测试构建：以「回落软件编码器」的策略跑完整链路，让没有硬编的机器
/// 也能验证管线本身（解码/Annex-B/封装/时间戳）。硬件与否由探测测试另行验证。
/// 门控对齐 [`crate::transcode_h264_for_tests`]（壳 e2e 走 test-support）。
#[cfg(any(test, feature = "test-support"))]
pub fn transcode_h264_for_tests(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    let demuxed = hongguo_media::demux::demux_file(req.input).ok()?;
    let video = demuxed.video_track()?;
    if !hongguo_media::hevc::is_hevc(&video.info.codec) {
        return None;
    }
    Some(run_on_mf_thread(|| {
        run(req, &demuxed, &video.info, EncoderKind::Software)
    }))
}

// ————————————————————————————————————————————————————————————
// 码率
// ————————————————————————————————————————————————————————————

// 码率标定 bitrate_for 收敛到 super::video（mf/vt 两侧原本逐字节相同）。

// ————————————————————————————————————————————————————————————
// 编排（与 vt 侧逐段对应）
// ————————————————————————————————————————————————————————————

/// 下一帧的显示时间：按升序时间轴逐帧取，重复/回退按半帧顶开。
fn next_pts(sorted_pts: &[f64], cursor: &mut usize, last: &mut Option<f64>, frame_dur: f64) -> f64 {
    let mut p = sorted_pts
        .get(*cursor)
        .copied()
        .unwrap_or_else(|| last.unwrap_or(0.0) + frame_dur);
    *cursor += 1;
    if let Some(prev) = *last
        && p <= prev
    {
        p = prev + frame_dur * 0.5;
    }
    *last = Some(p);
    p
}

/// 一帧解码输出 →（缩放）→ 编码 → 收割产物。
#[expect(
    clippy::too_many_arguments,
    reason = "参数表与平台 C API（虚表/会话属性）一一对应，硬拆参数结构反而失真"
)]
fn process_frame(
    frame: Nv12Frame,
    kind: EncoderKind,
    scale_to: Option<(usize, usize)>,
    fps: f64,
    frame_dur: f64,
    sorted_pts: &[f64],
    pts_cursor: &mut usize,
    last_pts: &mut Option<f64>,
    out_last: &mut Option<f64>,
    scaler: &mut Option<Scaler>,
    encoder: &mut Option<Encoder>,
    out_dims: &mut Option<(usize, usize)>,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    // 首帧才知道真实输出分辨率，缩放器与编码会话延迟到这里建
    if encoder.is_none() {
        let (src_w, src_h) = (frame.width, frame.height);
        let (out_w, out_h) = match scale_to {
            Some((w, h)) if (w, h) != (src_w, src_h) => {
                *scaler = Some(Scaler::new(
                    src_w,
                    src_h,
                    frame.stride,
                    w,
                    h,
                    fps_rate(fps),
                    1,
                )?);
                (w, h)
            }
            Some((w, h)) => (w, h),
            None => (src_w, src_h),
        };
        *encoder = Some(Encoder::new(
            out_w,
            out_h,
            fps_rate(fps),
            1,
            bitrate_for(out_w, out_h, fps),
            kind,
        )?);
        *out_dims = Some((out_w, out_h));
    }
    let frame = match scaler.as_mut() {
        Some(s) => s.scale(frame)?,
        None => frame,
    };
    let encoder = encoder.as_mut().expect("上一行刚建好");
    let pts = next_pts(sorted_pts, pts_cursor, last_pts, frame_dur);
    encoder.encode(frame, pts, frame_dur)?;
    emit_outputs(encoder, out_last, frame_dur, units, on_progress)
}

/// 收割编码器已就绪的输出。显示时间来自喂帧时间轴的配对（本身递增）；
/// 配不上（NaN）或异常回退时按输出侧自己的游标半帧顶开——**不能**与
/// `next_pts` 的喂入游标混用（见 run() 里的注释）。
fn emit_outputs(
    encoder: &mut Encoder,
    out_last: &mut Option<f64>,
    frame_dur: f64,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    for (raw, annexb, keyframe) in encoder.take_units()? {
        let pts = match *out_last {
            Some(prev) if !raw.is_finite() || raw <= prev => prev + frame_dur * 0.5,
            None if !raw.is_finite() => 0.0,
            _ => raw,
        };
        *out_last = Some(pts);
        units.push((pts, annexb, keyframe));
        if let Some(cb) = on_progress {
            cb(pts);
        }
    }
    Ok(())
}

/// 帧率 → 有理数分子（分母固定 1）。MF 只把它当码控提示，精度无关紧要。
fn fps_rate(fps: f64) -> u32 {
    fps.round().max(1.0) as u32
}

fn run(
    req: &PlatformRequest<'_>,
    demuxed: &hongguo_media::demux::Demuxed,
    video: &TrackInfo,
    kind: EncoderKind,
) -> AppResult<()> {
    let started = Instant::now();

    // 1) 源时间轴。解码走 SourceReader（内部自带参数集装配），帧以显示序
    //    出，逐帧对位升序时间轴。
    let fps = video.average_framerate().unwrap_or(25.0);
    let frame_dur = 1.0 / fps;
    let mut sorted_pts = video.sample_pts();
    if sorted_pts.is_empty() {
        sorted_pts = (0..video.samples.len()).map(|i| i as f64 / fps).collect();
    } else {
        sorted_pts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
    let scale_to = req
        .scale_to
        .map(|(w, h)| ((w & !1) as usize, (h & !1) as usize));

    // 2) 解码 → （缩放）→ 编码 流水
    let mut decoder = Decoder::new(req.input, video.width as usize, video.height as usize)?;
    let mut units: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let mut scaler: Option<Scaler> = None;
    let mut encoder: Option<Encoder> = None;
    let mut out_dims: Option<(usize, usize)> = None;
    let mut pts_cursor = 0usize;
    // 喂入时间轴与输出时间轴各用各的游标：混用会让输出兜底拿「喂到第几帧」
    // 当基准，把首输出时间错顶（实测 h264_mf 缓冲 33 帧后一次性吐出时，
    // 首帧 0.08 被顶成 0.78）。
    let mut last_pts: Option<f64> = None;
    let mut out_last: Option<f64> = None;

    while let Some(f) = decoder.next_frame()? {
        process_frame(
            f,
            kind,
            scale_to,
            fps,
            frame_dur,
            &sorted_pts,
            &mut pts_cursor,
            &mut last_pts,
            &mut out_last,
            &mut scaler,
            &mut encoder,
            &mut out_dims,
            &mut units,
            req.on_progress,
        )?;
    }

    // 3) 冲出编码器尾帧
    let mut encoder = match encoder {
        Some(e) => e,
        None => return Err(AppError::Media("HEVC 解码没有产出任何帧".into())),
    };
    encoder.finish()?;
    emit_outputs(
        &mut encoder,
        &mut out_last,
        frame_dur,
        &mut units,
        req.on_progress,
    )?;

    if units.is_empty() {
        return Err(AppError::Media("平台硬编没有产出任何帧".into()));
    }

    // 4) 音轨直通 + 封装（与其他后端同一套代码，faststart）
    let (out_w, out_h) = out_dims.unwrap_or((video.width as usize, video.height as usize));
    hongguo_media::mux::with_audio(req.input, demuxed, req.output, &units, out_w, out_h)?;

    log::info!(
        "[Platform/mf] HEVC→H.264 完成：{} 帧（{out_w}x{out_h}），耗时 {:?}",
        units.len(),
        started.elapsed()
    );
    Ok(())
}
