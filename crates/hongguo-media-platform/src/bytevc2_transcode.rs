//! ByteVC2 明文 MP4 → H.264。解码只走 `hongguo-bytevc2`，编码走平台 H.264 会话。
//! 这条路径不调用 ffmpeg / librempeg。

use std::time::Instant;

use hongguo_common::{AppError, AppResult};
use hongguo_media::mux::{AudioTrack, GrowingH264};
use hongguo_media_platform_api::PlatformRequest;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
use crate::platform::{YuvEncoder, imp_thread};

/// 把一集 ByteVC2 明文转成可播的 H.264 MP4。源不是 ByteVC2 时返回错误，不落回 ffmpeg。
pub fn transcode_bytevc2(req: &PlatformRequest<'_>) -> AppResult<()> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
    {
        let result = imp_thread(|| drive(req));
        if result.is_err() {
            let _ = std::fs::remove_file(req.output);
        }
        result
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        let _ = req;
        Err(AppError::Media("当前平台不能把 ByteVC2 编成 H.264".into()))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
struct LiveFile<'a> {
    writer: Option<GrowingH264>,
    audio: Option<&'a AudioTrack>,
    samples: &'a [(f64, Vec<u8>)],
    cursor: usize,
    size: Option<(usize, usize)>,
    timeline: f64,
    announced: bool,
    ready_ns: u128,
    prefix_samples: u32,
    prefix_bytes: u64,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
fn drive(req: &PlatformRequest<'_>) -> AppResult<()> {
    let started = Instant::now();
    let demuxed = hongguo_media::demux::demux_file(req.input)?;
    let source = hongguo_bytevc2::open_plaintext(req.input)?;
    let fps = source.fps();
    let frame_dur = 1.0 / fps;
    let scale_to = req
        .scale_to
        .map(|(w, h)| ((w & !1) as usize, (h & !1) as usize));
    let audio = hongguo_media::mux::collect(req.input, &demuxed)?;
    let timeline = timeline_secs(&demuxed, audio.as_ref(), frame_dur);
    let mut encoder = YuvEncoder::new(scale_to, fps, req.rate);
    let mut live = LiveFile {
        writer: None,
        audio: audio.as_ref(),
        samples: audio.as_ref().map(|t| t.samples.as_slice()).unwrap_or(&[]),
        cursor: 0,
        size: None,
        timeline,
        announced: false,
        ready_ns: 0,
        prefix_samples: 0,
        prefix_bytes: 0,
    };
    let mut frames = 0usize;
    let mut slot = 0usize;
    let mut slot_times = demuxed
        .video_track()
        .map(|track| track.info.sample_pts())
        .unwrap_or_default();
    slot_times.retain(|pts| pts.is_finite() && *pts >= 0.0);
    slot_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut ready: Vec<hongguo_bytevc2::DecodedFrame> = Vec::new();
    let stage_log = std::env::var("HONGGUO_BVC2_STAGE_LOG").ok();
    let mut push_ns = 0u128;
    let loop_started = Instant::now();
    {
        let mut pass = Pass {
            encoder: &mut encoder,
            live: &mut live,
            stage_log: &stage_log,
            push_ns: &mut push_ns,
            frame_dur,
            req,
            started,
        };
        source.decode(|frame| {
            ready.push(frame);
            frames += feed_ready(&mut pass, &mut ready, &slot_times, &mut slot)?;
            Ok(())
        })?;
        ready
            .sort_by_key(|frame| hongguo_bytevc2::presentation_tick(frame.pts.unwrap_or(f64::MAX)));
        frames += feed_sorted(&mut pass, &mut ready)?;
    }
    let loop_ns = loop_started.elapsed().as_nanos();
    if frames == 0 {
        return Err(AppError::Media("ByteVC2 解码没有产出任何帧".into()));
    }
    let flush_started = Instant::now();
    let encoded = encoder.finish()?;
    let flush_ns = flush_started.elapsed().as_nanos();
    if live.size.is_none() {
        live.size = Some((encoded.width, encoded.height));
    }
    let mux_started = Instant::now();
    if !encoded.units.is_empty() {
        write_gop(&mut live, &encoded.units, None, req, started)?;
    }
    let mux_ns = mux_started.elapsed().as_nanos();
    if !live.announced {
        return Err(AppError::Media("ByteVC2 没有写出可播片段".into()));
    }
    let width = encoded.width;
    let height = encoded.height;
    if let Some(path) = stage_log {
        let decode_ns = loop_ns.saturating_sub(push_ns);
        let body = format!(
            "{{\"frames\":{frames},\"width\":{width},\"height\":{height},\"fps\":{fps},\"loop_ns\":{loop_ns},\"decode_ns\":{decode_ns},\"encode_push_ns\":{push_ns},\"encode_flush_ns\":{flush_ns},\"mux_ns\":{mux_ns},\"prefix_ready_ns\":{},\"prefix_video_samples\":{},\"prefix_bytes\":{},\"wall_ns\":{}}}\n",
            live.ready_ns,
            live.prefix_samples,
            live.prefix_bytes,
            started.elapsed().as_nanos()
        );
        std::fs::write(&path, body).map_err(|e| AppError::Io(format!("阶段日志写入失败: {e}")))?;
    }
    log::info!(
        "[ByteVC2] 解帧并编成 H.264：{frames} 帧（{width}x{height}，{fps:.2} fps），耗时 {:?}",
        started.elapsed()
    );
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
struct Pass<'a, 'b> {
    encoder: &'a mut YuvEncoder,
    live: &'a mut LiveFile<'b>,
    stage_log: &'a Option<String>,
    push_ns: &'a mut u128,
    frame_dur: f64,
    req: &'a PlatformRequest<'b>,
    started: Instant,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
fn feed_ready(
    pass: &mut Pass<'_, '_>,
    ready: &mut Vec<hongguo_bytevc2::DecodedFrame>,
    slot_times: &[f64],
    slot: &mut usize,
) -> AppResult<usize> {
    let mut fed = 0usize;
    while *slot < slot_times.len() {
        let want = hongguo_bytevc2::presentation_tick(slot_times[*slot]);
        let Some(pos) = ready.iter().position(|frame| {
            frame
                .pts
                .is_some_and(|pts| hongguo_bytevc2::presentation_tick(pts) == want)
        }) else {
            break;
        };
        let frame = ready.swap_remove(pos);
        let pts = frame.pts.unwrap_or(slot_times[*slot]);
        push_frame(pass, &frame, pts)?;
        *slot += 1;
        fed += 1;
    }
    drain_gops(pass)?;
    Ok(fed)
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
fn feed_sorted(
    pass: &mut Pass<'_, '_>,
    ready: &mut Vec<hongguo_bytevc2::DecodedFrame>,
) -> AppResult<usize> {
    let pending = std::mem::take(ready);
    for (index, frame) in pending.iter().enumerate() {
        let pts = frame.pts.unwrap_or(index as f64 * pass.frame_dur);
        push_frame(pass, frame, pts)?;
    }
    drain_gops(pass)?;
    Ok(pending.len())
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
fn push_frame(
    pass: &mut Pass<'_, '_>,
    frame: &hongguo_bytevc2::DecodedFrame,
    pts: f64,
) -> AppResult<()> {
    let push_started = pass.stage_log.as_ref().map(|_| Instant::now());
    pass.encoder.push(&hongguo_media_platform_api::I420View {
        y: &frame.y,
        u: &frame.u,
        v: &frame.v,
        width: frame.width,
        height: frame.height,
        pts,
        dur: pass.frame_dur,
    })?;
    if let Some(push_started) = push_started {
        *pass.push_ns += push_started.elapsed().as_nanos();
    }
    if let Some(cb) = pass.req.on_progress {
        cb(pts);
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
const PUBLISH_FRAMES: usize = 30;

fn drain_gops(pass: &mut Pass<'_, '_>) -> AppResult<()> {
    if pass.req.is_cancelled.is_some_and(|cancelled| cancelled()) {
        return Err(AppError::Cancelled);
    }
    if pass.live.size.is_none() {
        pass.live.size = pass.encoder.coded_size();
    }
    loop {
        let require_sync = pass.live.writer.is_none();
        let Some((gop, next_pts)) = pass.encoder.take_publish_run(PUBLISH_FRAMES, require_sync)
        else {
            break;
        };
        write_gop(pass.live, &gop, next_pts, pass.req, pass.started)?;
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
fn write_gop(
    live: &mut LiveFile<'_>,
    gop: &[(f64, Vec<u8>, bool)],
    end_pts: Option<f64>,
    req: &PlatformRequest<'_>,
    started: Instant,
) -> AppResult<()> {
    if gop.is_empty() || (live.writer.is_none() && !gop[0].2) {
        return Err(AppError::Media("H.264 片段缺少同步样本".into()));
    }
    let (width, height) = live
        .size
        .ok_or_else(|| AppError::Media("编码器还没有分辨率".into()))?;
    if live.writer.is_none() {
        live.writer = Some(GrowingH264::create(
            req.output,
            width,
            height,
            live.audio,
            live.timeline,
        )?);
    }
    let audio = take_audio(live.samples, &mut live.cursor, end_pts).to_vec();
    let bytes = live
        .writer
        .as_mut()
        .expect("片段文件刚打开")
        .push(gop, &audio)?;
    if let Some(cb) = req.on_committed {
        cb(bytes);
    }
    if live.announced {
        return Ok(());
    }
    live.announced = true;
    live.ready_ns = started.elapsed().as_nanos();
    live.prefix_samples = gop.len() as u32;
    live.prefix_bytes = bytes;
    log::info!(
        "[ByteVC2] 首段可播：{} 帧，{bytes} 字节",
        live.prefix_samples
    );
    if let Some(cb) = req.on_playable_prefix {
        cb(req.output);
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
fn take_audio<'a>(
    samples: &'a [(f64, Vec<u8>)],
    cursor: &mut usize,
    end_pts: Option<f64>,
) -> &'a [(f64, Vec<u8>)] {
    let start = (*cursor).min(samples.len());
    let stop = match end_pts {
        Some(limit) => {
            let rest = samples[start..]
                .iter()
                .position(|(pts, _)| *pts >= limit)
                .unwrap_or(samples.len() - start);
            start + rest
        }
        None => samples.len(),
    };
    *cursor = stop;
    &samples[start..stop]
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
fn timeline_secs(
    demuxed: &hongguo_media::demux::Demuxed,
    audio: Option<&AudioTrack>,
    frame_dur: f64,
) -> f64 {
    let video_end = demuxed
        .video_track()
        .and_then(|track| {
            track
                .info
                .sample_pts()
                .last()
                .copied()
                .map(|pts| pts.max(0.0) + frame_dur)
        })
        .unwrap_or(frame_dur);
    let audio_end = audio
        .and_then(|track| track.samples.last().map(|(pts, _)| *pts))
        .unwrap_or(0.0);
    video_end.max(audio_end).max(frame_dur)
}
