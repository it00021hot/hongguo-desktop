//! ByteVC2 明文 MP4 → H.264。解码只走 `hongguo-bytevc2`，编码走平台 H.264 会话。
//! 这条路径不调用 ffmpeg / librempeg。

use std::time::Instant;

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform_api::PlatformRequest;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
use crate::platform::imp_thread;

/// 把一集 ByteVC2 明文转成可播的 H.264 MP4。源不是 ByteVC2 时返回错误，不落回 ffmpeg。
pub fn transcode_bytevc2(req: &PlatformRequest<'_>) -> AppResult<()> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
    {
        imp_thread(|| drive(req))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        let _ = req;
        Err(AppError::Media("当前平台不能把 ByteVC2 编成 H.264".into()))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
fn drive(req: &PlatformRequest<'_>) -> AppResult<()> {
    use crate::platform::YuvEncoder;

    let started = Instant::now();
    let demuxed = hongguo_media::demux::demux_file(req.input)?;
    let source = hongguo_bytevc2::open_plaintext(req.input)?;
    let fps = source.fps();
    let scale_to = req
        .scale_to
        .map(|(w, h)| ((w & !1) as usize, (h & !1) as usize));
    let mut encoder = YuvEncoder::new(scale_to, fps, req.rate);
    let mut frames = 0usize;
    let dur = 1.0 / fps;
    source.decode(|frame| {
        let pts = frames as f64 * dur;
        encoder.push(&hongguo_media_platform_api::I420View {
            y: &frame.y,
            u: &frame.u,
            v: &frame.v,
            width: frame.width,
            height: frame.height,
            pts,
            dur,
        })?;
        if let Some(cb) = req.on_progress {
            cb(pts);
        }
        frames += 1;
        Ok(())
    })?;
    if frames == 0 {
        return Err(AppError::Media("ByteVC2 解码没有产出任何帧".into()));
    }
    let encoded = encoder.finish()?;
    let width = encoded.width;
    let height = encoded.height;
    hongguo_media::mux::with_audio(
        req.input,
        &demuxed,
        req.output,
        &encoded.units,
        width,
        height,
    )?;
    log::info!(
        "[ByteVC2] 解帧并编成 H.264：{frames} 帧（{width}x{height}，{fps:.2} fps），耗时 {:?}",
        started.elapsed()
    );
    Ok(())
}
