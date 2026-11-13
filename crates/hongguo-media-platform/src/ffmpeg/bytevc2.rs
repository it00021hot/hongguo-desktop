//! ByteVC2 → H.264 via ffmpeg when the local build can decode `bvc2` (librempeg / patched ffmpeg).
//!
//! Built-in vvdec + platform H.264 is the default archive route.
//! This ffmpeg/librempeg `bvc2` path is a debug fallback only: it stays off unless
//! `HONGGUO_ALLOW_FFMPEG=1` is set (and `HONGGUO_NO_FFMPEG` is not). oxideav remains an
//! optional decode backend.

use hongguo_common::AppResult;

use super::probe::{ffmpeg_path, h264_encoder};
use crate::platform::PlatformRequest;

/// Try ffmpeg for ByteVC2. Returns `None` when the debug fallback is off (default) or
/// ffmpeg/encoder is unavailable, so the caller can use the in-process vvdec path.
pub fn try_transcode_bytevc2(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    let encoder = h264_encoder()?;
    ffmpeg_path()?;
    let archive = super::transcode::TranscodeRequest {
        input: req.input,
        output: req.output,
        scale_to: req.scale_to,
        on_progress: req.on_progress,
    };
    Some(super::transcode::transcode_with_ffmpeg(&archive, &encoder))
}
