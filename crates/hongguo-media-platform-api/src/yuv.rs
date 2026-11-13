//! 平台 HEVC 编码器吃的已解码帧，以及它吐出的 Annex-B 样本。
//! ByteVC2 解帧在 `hongguo-bytevc2`，这里只规定编码会话的入参和出参。

/// 一帧紧密 8-bit 4:2:0，显示序。
pub struct I420View<'a> {
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub pts: f64,
    pub dur: f64,
}

/// 编码会话收尾后的 HEVC Annex-B（显示时间、载荷、是否关键帧）。
pub struct EncodedVideo {
    pub width: usize,
    pub height: usize,
    pub units: Vec<(f64, Vec<u8>, bool)>,
}
