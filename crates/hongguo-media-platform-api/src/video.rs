//! 视频契约：编码枚举与码率标定（平台实现与业务调用方共同依赖）。

/// 视频编码：解码侧要按它选 MFT / VT 会话与序列头形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    /// H.265（HEVC）：序列头是 hvcC。
    Hevc,
    /// H.264（AVC）：序列头是 avcC。
    H264,
}

/// 码率标定：VT 没有 CRF、MF 的 MFT 同理，按「0.12 bit/像素/帧」给出与
/// libx264 `-crf 23` 同档的码率（1080p25 ≈ 6.2 Mbps；中低复杂度近真人剧
/// VMAF 略优于 crf23，高运动内容 CBR 固定码率天然吃亏，0.12 系数可收窄
/// 差距）。再偏差大时只调这里的系数。
pub fn bitrate_for(width: usize, height: usize, fps: f64) -> i32 {
    let bps = width as f64 * height as f64 * fps.max(1.0) * 0.12;
    bps.clamp(800_000.0, 12_000_000.0) as i32
}
