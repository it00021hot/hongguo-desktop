//! 择优域数据模型（流的比较指标与挑中的流）。

use std::cmp::Ordering;

/// 本机解码能力对选流的约束（语义对齐官方 TTVideoEngine `CodecStrategy`：
/// 能力决定「谁参赛、怎么排」，不把解不了的流硬喂给解码器）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderCapability {
    /// 系统 HEVC 解码在位：现状行为，清晰度优先。
    Full,
    /// 无 HEVC 解码（Windows 缺「HEVC 视频扩展」）：h264 最优先、未知编码次之、
    /// HEVC 系末选；整集只有 HEVC 时仍取流，黑屏后由转码兜底接住。
    NoHevc,
}

impl DecoderCapability {
    /// 按本机探测结论选模式（`true` = 系统 HEVC 解码在位）。
    pub fn from_hevc(playable: bool) -> Self {
        if playable { Self::Full } else { Self::NoHevc }
    }
}

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
    /// 是否处于无 HEVC 模式（决定 [`Ord`] 用哪套次序）
    pub no_hevc: bool,
    /// 无 HEVC 模式的编码层级（0=h264 恒可解，1=未知编码，2=HEVC 系末选）；
    /// Full 模式恒 0，不参与比较
    pub codec_tier: u8,
}

/// 择优顺序按模式分两套：**Full** = 清晰度 > 像素 > 编码器偏好（rank 小者优，反向比较）> 码率；
/// **NoHevc** = 编码可解性 > 清晰度 > 像素 > 码率（官方维度序列语义，层级同样反向比较）。
impl Ord for StreamScore {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.no_hevc {
            other
                .codec_tier
                .cmp(&self.codec_tier)
                .then_with(|| self.def_num.cmp(&other.def_num))
                .then_with(|| self.pixels.cmp(&other.pixels))
                .then_with(|| self.bitrate.cmp(&other.bitrate))
        } else {
            self.def_num
                .cmp(&other.def_num)
                .then_with(|| self.pixels.cmp(&other.pixels))
                .then_with(|| other.codec_rank.cmp(&self.codec_rank))
                .then_with(|| self.bitrate.cmp(&other.bitrate))
        }
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
