//! 视频契约：编码枚举与码率标定（平台实现与业务调用方共同依赖）。

const ARCHIVE_BITS_PER_PIXEL: f64 = 0.12;
const MIN_BITRATE: f64 = 800_000.0;
const MAX_BITRATE: f64 = 12_000_000.0;

/// 归档码率标定（1080p25 ≈ 6.2 Mbps）。
pub fn bitrate_for(width: usize, height: usize, fps: f64) -> i32 {
    let bps = width as f64 * height as f64 * fps.max(1.0) * ARCHIVE_BITS_PER_PIXEL;
    bps.clamp(MIN_BITRATE, MAX_BITRATE) as i32
}
