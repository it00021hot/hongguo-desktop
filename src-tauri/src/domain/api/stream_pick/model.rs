//! 择优域数据模型（流的比较指标与挑中的流）。

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamScore {
    /// 清晰度等级
    pub def_num: u32,
    /// 像素数
    pub pixels: u32,
    /// 编码器偏好（**越小越优先**：h265/h264 系 > bytevc1 > 其它）
    pub codec_rank: u8,
    /// 码率
    pub bitrate: u32,
}

/// 择优顺序：清晰度 > 像素 > 编码器偏好 > 码率。
///
/// 编码器一项要**反向**比较——rank 越小越好，而 `Ord` 的语义是「大者胜」，
/// 直接用 derive 会把 bytevc1 排在 h265 前面。
impl Ord for StreamScore {
    fn cmp(&self, other: &Self) -> Ordering {
        self.def_num
            .cmp(&other.def_num)
            .then_with(|| self.pixels.cmp(&other.pixels))
            .then_with(|| other.codec_rank.cmp(&self.codec_rank))
            .then_with(|| self.bitrate.cmp(&other.bitrate))
    }
}

impl PartialOrd for StreamScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 挑到的一条流。
pub struct PlayStream {
    pub url: String,
    pub spade: String,
    pub codec: Option<String>,
    /// 实际选中的档位。发生回退时它是自动挑的那档，不是请求的那档。
    pub definition: u32,
}
