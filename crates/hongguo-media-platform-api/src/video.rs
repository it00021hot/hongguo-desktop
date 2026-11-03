//! 视频契约：编码枚举与码率标定（平台实现与业务调用方共同依赖）。

/// 视频编码：解码侧要按它选 MFT / VT 会话与序列头形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    /// H.265（HEVC）：序列头是 hvcC。
    Hevc,
    /// H.264（AVC）：序列头是 avcC。
    H264,
}

/// 码率档：合并归档要接近源观感；兼容播放只要本机可播，体积与编码耗时优先。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RateProfile {
    /// 与 libx264 `-crf 23` 同档（0.12 bit/像素/帧）。合并产物用。
    #[default]
    Archive,
    /// 约 crf 28 档（0.07 bit/像素/帧）。兼容播放兜底用——实测归档档把 1080p 一分钟短剧胀到 ~60MB。
    Playback,
}

impl RateProfile {
    /// 每像素每帧的目标比特数。
    fn bits_per_pixel(self) -> f64 {
        match self {
            Self::Archive => 0.12,
            Self::Playback => 0.07,
        }
    }
}

/// 码率标定（归档档）：VT 没有 CRF、MF 的 MFT 同理，按 [`RateProfile::Archive`] 给出与 libx264 `-crf 23` 同档的码率（1080p25 ≈ 6.2 Mbps）。
pub fn bitrate_for(width: usize, height: usize, fps: f64) -> i32 {
    bitrate_for_profile(width, height, fps, RateProfile::Archive)
}

/// 按用途标定码率。上限随档位收：播放档封顶 4 Mbps（720p30 ≈ 1.9 Mbps），避免竖屏短剧再涨回归档体积。
pub fn bitrate_for_profile(width: usize, height: usize, fps: f64, profile: RateProfile) -> i32 {
    let bps = width as f64 * height as f64 * fps.max(1.0) * profile.bits_per_pixel();
    let (lo, hi) = match profile {
        RateProfile::Archive => (800_000.0, 12_000_000.0),
        RateProfile::Playback => (400_000.0, 4_000_000.0),
    };
    bps.clamp(lo, hi) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_bitrate_is_lighter_than_archive() {
        let archive = bitrate_for(1920, 1080, 25.0);
        let play = bitrate_for_profile(1920, 1080, 25.0, RateProfile::Playback);
        assert!(play < archive, "播放 {play} 应低于归档 {archive}");
        // 1080p25 @ 0.07 bpp ≈ 3.63 Mbps，仍低于播放档封顶 4 Mbps
        assert!((3_200_000..=4_000_000).contains(&play), "实际 {play}");
        // 720p30 播放档 ≈ 1.94 Mbps
        let p720 = bitrate_for_profile(1280, 720, 30.0, RateProfile::Playback);
        assert!((1_700_000..=2_200_000).contains(&p720), "实际 {p720}");
    }
}
