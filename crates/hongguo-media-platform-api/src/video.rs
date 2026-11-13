//! 视频契约：编码枚举与码率标定（平台实现与业务调用方共同依赖）。

/// 视频编码：解码侧要按它选 MFT / VT 会话与序列头形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    /// H.265（HEVC）：序列头是 hvcC。
    Hevc,
}

const ARCHIVE_BITS_PER_PIXEL: f64 = 0.12;
const MIN_BITRATE: f64 = 800_000.0;
const MAX_BITRATE: f64 = 12_000_000.0;

/// 归档码率标定（1080p25 ≈ 6.2 Mbps）。
pub fn bitrate_for(width: usize, height: usize, fps: f64) -> i32 {
    let bps = width as f64 * height as f64 * fps.max(1.0) * ARCHIVE_BITS_PER_PIXEL;
    bps.clamp(MIN_BITRATE, MAX_BITRATE) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_bitrate_keeps_calibration_and_bounds() {
        assert_eq!(bitrate_for(1920, 1080, 25.0), 6_220_800);
        assert_eq!(bitrate_for(1280, 720, 30.0), 3_317_760);
        assert_eq!(bitrate_for(1, 1, 0.0), 800_000);
        assert_eq!(bitrate_for(7680, 4320, 60.0), 12_000_000);
    }
}
