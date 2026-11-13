//! ByteVC2 明文 MP4 → H.264。解码只走 `hongguo-bytevc2`，编码走平台 H.264 会话。
//! 这条路径不调用 ffmpeg / librempeg。

use std::time::Instant;

use hongguo_common::{AppError, AppResult};
use hongguo_media::mux::{AudioTrack, GrowingH264};
use hongguo_media_platform_api::PlatformRequest;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
use crate::platform::{YuvEncoder, imp_thread};

/// 把一集 ByteVC2 明文转成可播的 H.264 MP4。源不是 ByteVC2 时返回错误，不落回 ffmpeg。
pub fn transcode_bytevc2(req: &PlatformRequest<'_>) -> AppResult<()> {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    ))]
    {
        let result = imp_thread(|| drive(req));
        if result.is_err() {
            let _ = hongguo_common::offline_qa::qa_remove_file(req.output);
        }
        result
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        let _ = req;
        Err(AppError::Media("当前平台不能把 ByteVC2 编成 H.264".into()))
    }
}

/// 解码线程最多领先编码多少帧。够吸收编码器的瞬时抖动即可；1080p 一帧约 3 MB，
/// 队列再深只是多占内存。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
const DECODE_QUEUE_FRAMES: usize = 8;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
struct LiveFile<'a> {
    writer: Option<GrowingH264>,
    audio: Option<&'a AudioTrack>,
    samples: &'a [(f64, Vec<u8>)],
    cursor: usize,
    size: Option<(usize, usize)>,
    timeline: f64,
    video_end_secs: f64,
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
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
    let video_end_secs = hongguo_media::mux::track_end_secs(
        &demuxed
            .video_track()
            .ok_or_else(|| AppError::Media("缺少源视频轨道".into()))?
            .info,
    )?;
    let timeline = video_end_secs.max(audio.as_ref().map_or(0.0, |track| track.end_secs));
    let mut encoder = YuvEncoder::new(scale_to, fps);
    let mut live = LiveFile {
        writer: None,
        audio: audio.as_ref(),
        samples: audio.as_ref().map(|t| t.samples.as_slice()).unwrap_or(&[]),
        cursor: 0,
        size: None,
        timeline,
        video_end_secs,
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
        };
        // 解码与编码各占一个线程：解码线程只管出帧，本线程（平台编码器要求的线程）送编码并封装。
        // 原来两者在同一个回调里串行，编码器吃帧时解码核的线程池是空转的。
        pump(
            DECODE_QUEUE_FRAMES,
            |emit| source.decode(emit),
            |frame| {
                ready.push(frame);
                frames += feed_ready(&mut pass, &mut ready, &slot_times, &mut slot)?;
                Ok(())
            },
        )?;
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
        write_gop(&mut live, &encoded.units, None, req)?;
    }
    let mux_ns = mux_started.elapsed().as_nanos();
    if live.writer.is_none() {
        return Err(AppError::Media("ByteVC2 没有写出归档片段".into()));
    }
    let width = encoded.width;
    let height = encoded.height;
    let wall_ns = started.elapsed().as_nanos();
    let media_secs = (frames as f64) / if fps > 0.0 { fps } else { 30.0 };
    let wall_secs = (wall_ns as f64) / 1_000_000_000.0;
    let realtime_x = if wall_secs > 0.0 {
        media_secs / wall_secs
    } else {
        0.0
    };
    let decoder = hongguo_bytevc2::active_decoder_name();
    if let Some(path) = stage_log {
        // 解码已挪到独立线程，这个值现在是「主循环里没花在送编码上的时间」（等帧 + 封装），
        // 不再等于纯解码耗时；字段名保留以兼容既有的阶段日志读取方。
        let decode_ns = loop_ns.saturating_sub(push_ns);
        let body = format!(
            "{{\"frames\":{frames},\"width\":{width},\"height\":{height},\"fps\":{fps},\"decoder\":\"{decoder}\",\"media_secs\":{media_secs:.6},\"realtime_x\":{realtime_x:.4},\"loop_ns\":{loop_ns},\"decode_ns\":{decode_ns},\"encode_push_ns\":{push_ns},\"encode_flush_ns\":{flush_ns},\"mux_ns\":{mux_ns},\"wall_ns\":{wall_ns}}}\n",
        );
        hongguo_common::offline_qa::qa_write(std::path::Path::new(&path), body.as_bytes())
            .map_err(|e| AppError::Io(format!("阶段日志写入失败: {e}")))?;
    }
    log::info!(
        "[ByteVC2] 解帧并编成 H.264：{frames} 帧（{width}x{height}，{fps:.2} fps，decoder={decoder}），耗时 {:?} ≈{realtime_x:.2}× realtime（无 ffmpeg 进程）",
        started.elapsed()
    );
    Ok(())
}

/// 在独立线程上跑 `produce`，本线程按产出顺序逐个 `consume`，中间是深度 `depth` 的有界队列。
///
/// - `consume` 出错：停止接收，`produce` 的下一次 `emit` 得到 `Cancelled` 而收手，返回 `consume` 的错误。
/// - `produce` 出错：先把已入队的帧消费完，再返回它的错误。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
fn pump<T: Send>(
    depth: usize,
    produce: impl FnOnce(&mut dyn FnMut(T) -> AppResult<()>) -> AppResult<()> + Send,
    mut consume: impl FnMut(T) -> AppResult<()>,
) -> AppResult<()> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<T>(depth);
    std::thread::scope(|scope| {
        let producer = std::thread::Builder::new()
            .name("bytevc2-decode".into())
            .spawn_scoped(scope, move || {
                produce(&mut |item| tx.send(item).map_err(|_| AppError::Cancelled))
            })
            .map_err(|e| AppError::Media(format!("ByteVC2 解码线程起不来: {e}")))?;
        let mut consumed: AppResult<()> = Ok(());
        for item in &rx {
            if let Err(e) = consume(item) {
                consumed = Err(e);
                break;
            }
        }
        // 先放掉接收端再等线程：否则生产方可能永远堵在满队列的 send 上。
        drop(rx);
        let produced = producer
            .join()
            .map_err(|_| AppError::Media("ByteVC2 解码线程异常退出".into()))?;
        // 消费侧的错误是真因；此时生产侧报的只是「接收端没了」。
        consumed?;
        produced
    })
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
struct Pass<'a, 'b> {
    encoder: &'a mut YuvEncoder,
    live: &'a mut LiveFile<'b>,
    stage_log: &'a Option<String>,
    push_ns: &'a mut u128,
    frame_dur: f64,
    req: &'a PlatformRequest<'b>,
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
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

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
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

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
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

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
/// 保持归档首片段的既有批次。
const FIRST_PUBLISH_FRAMES: usize = 8;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
/// 后续归档片段的既有批次。
const PUBLISH_FRAMES: usize = 30;

fn drain_gops(pass: &mut Pass<'_, '_>) -> AppResult<()> {
    if pass.live.size.is_none() {
        pass.live.size = pass.encoder.coded_size();
    }
    loop {
        let first = pass.live.writer.is_none();
        let need = if first {
            FIRST_PUBLISH_FRAMES
        } else {
            PUBLISH_FRAMES
        };
        let Some((gop, next_pts)) = pass.encoder.take_publish_run(need, first) else {
            break;
        };
        let end = next_pts.ok_or_else(|| AppError::Media("编码片段缺少后继帧时间".into()))?;
        write_gop(pass.live, &gop, Some(end), pass.req)?;
    }
    Ok(())
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
fn write_gop(
    live: &mut LiveFile<'_>,
    gop: &[(f64, Vec<u8>, bool)],
    end_pts: Option<f64>,
    req: &PlatformRequest<'_>,
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
    let timing = hongguo_media::mux::FragmentTiming {
        video_end_secs: end_pts.unwrap_or(live.video_end_secs),
        audio_end_secs: if audio.is_empty() {
            None
        } else {
            live.samples
                .get(live.cursor)
                .map(|(pts, _)| *pts)
                .or_else(|| live.audio.map(|track| track.end_secs))
        },
    };
    live.writer
        .as_mut()
        .expect("片段文件刚打开")
        .push(gop, &audio, timing)?;
    Ok(())
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
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

#[cfg(all(
    test,
    any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    )
))]
#[path = "bytevc2_audio_publish_tests.rs"]
mod audio_publish_tests;

#[cfg(all(
    test,
    any(target_os = "macos", target_os = "windows", target_os = "android")
))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn pump_delivers_every_item_in_order() {
        let mut got = Vec::new();
        pump(
            2,
            |emit| {
                for i in 0..200u32 {
                    emit(i)?;
                }
                Ok(())
            },
            |i| {
                got.push(i);
                Ok(())
            },
        )
        .expect("全部送达");
        assert_eq!(got, (0..200).collect::<Vec<_>>());
    }

    #[test]
    fn pump_consumer_error_stops_the_producer_without_deadlock() {
        // 队列深度 1、生产方想发 1000 个：消费方第 3 个就报错，生产方必须很快收手而不是堵死。
        let emitted = AtomicUsize::new(0);
        let err = pump(
            1,
            |emit| {
                for i in 0..1000u32 {
                    emit(i)?;
                    emitted.fetch_add(1, Ordering::SeqCst);
                }
                Ok(())
            },
            |i| {
                if i == 2 {
                    return Err(AppError::Media("编码失败".into()));
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, AppError::Media(ref m) if m == "编码失败"),
            "实际 {err}"
        );
        assert!(emitted.load(Ordering::SeqCst) < 10, "生产方应随即停下");
    }

    #[test]
    fn pump_cancellation_from_the_consumer_is_reported_as_cancelled() {
        let err = pump(
            4,
            |emit| loop {
                emit(0u8)?;
            },
            |_| Err(AppError::Cancelled),
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Cancelled), "实际 {err}");
    }

    #[test]
    fn pump_producer_error_surfaces_after_queued_items_are_consumed() {
        let mut got = Vec::new();
        let err = pump(
            8,
            |emit| {
                emit(1u32)?;
                emit(2)?;
                Err(AppError::Media("码流损坏".into()))
            },
            |i| {
                got.push(i);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(got, [1, 2], "出错前已解出的帧不能丢");
        assert!(
            matches!(err, AppError::Media(ref m) if m == "码流损坏"),
            "实际 {err}"
        );
    }
}
