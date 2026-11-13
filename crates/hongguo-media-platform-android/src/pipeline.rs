//! MediaCodec 转码编排：HEVC→H.264，音轨经 [`hongguo_media::mux::with_audio`] 直通。

use std::time::Instant;

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform_api::PlatformRequest;
use hongguo_media_platform_api::video::bitrate_for;
use hongguo_video::sample_table::TrackInfo;

use super::decode::{HevcDecoder, Nv12Frame};
use super::encode::H264Encoder;
use super::probe;
use super::yuv::scale_nv12;

/// 平台编码层执行转码；HEVC 以外或编码器不可用返回 `None`。
pub fn transcode_h264(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    if !probe::encoder_available() || !probe::hevc_decoder_present() {
        return None;
    }
    let demuxed = hongguo_media::demux::demux_file(req.input).ok()?;
    let video = demuxed.video_track()?;
    if !hongguo_media::hevc::is_hevc(&video.info.codec) {
        return None;
    }
    Some(run(req, &demuxed, &video.info))
}

fn run(
    req: &PlatformRequest<'_>,
    demuxed: &hongguo_media::demux::Demuxed,
    video: &TrackInfo,
) -> AppResult<()> {
    let started = Instant::now();
    let (raw_sets, length_size) = hongguo_media::hevc::read_parameter_set_nalus(req.input, video)?;
    let mut csd = Vec::new();
    for nalu in &raw_sets {
        csd.extend_from_slice(&[0, 0, 0, 1]);
        csd.extend_from_slice(nalu);
    }
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

    let mut decoder = HevcDecoder::new(video.width as usize, video.height as usize, &csd)?;
    let mut encoder: Option<H264Encoder> = None;
    let mut units: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let mut out_dims = (video.width as usize, video.height as usize);
    let mut out_last: Option<f64> = None;

    let n = video.samples.len();
    for (i, &(offset, size)) in video.samples.iter().enumerate() {
        let raw = hongguo_media::transcode::read_range(req.input, offset, size)?;
        if raw.is_empty() {
            continue;
        }
        let annexb = hongguo_media::hevc::to_annexb(&raw, length_size);
        let pts = sorted_pts.get(i).copied().unwrap_or(i as f64 * frame_dur);
        let pts_us = (pts * 1_000_000.0).round() as i64;
        let eos = i + 1 == n;
        decoder.push(&annexb, pts_us, eos)?;
        feed_frames(
            &mut decoder.pull()?,
            &mut encoder,
            &mut units,
            &mut out_dims,
            &mut out_last,
            scale_to,
            fps,
            frame_dur,
            req.on_progress,
            false,
        )?;
    }

    feed_frames(
        &mut decoder.pull()?,
        &mut encoder,
        &mut units,
        &mut out_dims,
        &mut out_last,
        scale_to,
        fps,
        frame_dur,
        req.on_progress,
        false,
    )?;

    let Some(enc) = encoder.as_mut() else {
        return Err(AppError::Media("MediaCodec 没有产出任何帧".into()));
    };
    enc.push_nv12(&[], 0, true)?;
    emit_units(
        &mut enc.pull()?,
        &mut units,
        &mut out_last,
        frame_dur,
        req.on_progress,
    );

    if units.is_empty() {
        return Err(AppError::Media("MediaCodec 没有产出任何帧".into()));
    }
    if !units[0].2 {
        return Err(AppError::Media("MediaCodec 首帧不是同步样本".into()));
    }

    let (out_w, out_h) = out_dims;
    hongguo_media::mux::with_audio(req.input, demuxed, req.output, &units, out_w, out_h)?;
    log::info!(
        "[Platform/android] HEVC→H.264 完成：{} 帧（{out_w}x{out_h}），耗时 {:?}",
        units.len(),
        started.elapsed()
    );
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "与 windows/macos 管线同构：缩放/码率/时间轴游标一次喂齐"
)]
fn feed_frames(
    frames: &mut Vec<Nv12Frame>,
    encoder: &mut Option<H264Encoder>,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    out_dims: &mut (usize, usize),
    out_last: &mut Option<f64>,
    scale_to: Option<(usize, usize)>,
    fps: f64,
    frame_dur: f64,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
    _eos: bool,
) -> AppResult<()> {
    for frame in frames.drain(..) {
        let (fw, fh) = (frame.width, frame.height);
        let (data, w, h) = match scale_to {
            Some((tw, th)) if (tw, th) != (fw, fh) => {
                (scale_nv12(&frame.data, fw, fh, tw, th)?, tw, th)
            }
            _ => (frame.data, fw, fh),
        };
        if encoder.is_none() {
            let bitrate = bitrate_for(w, h, fps);
            *encoder = Some(H264Encoder::new(w, h, fps, bitrate)?);
            *out_dims = (w, h);
        }
        let enc = encoder.as_mut().expect("刚建好");
        enc.push_nv12(&data, frame.pts_us, false)?;
        emit_units(&mut enc.pull()?, units, out_last, frame_dur, on_progress);
    }
    Ok(())
}

fn emit_units(
    drained: &mut Vec<(f64, Vec<u8>, bool)>,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    out_last: &mut Option<f64>,
    frame_dur: f64,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) {
    for (raw, annexb, keyframe) in drained.drain(..) {
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
}
