//! 择优域数据模型（流的比较指标与挑中的流）。

use std::cmp::Ordering;

/// 本机解码能力对选流的约束（语义对齐官方 TTVideoEngine `CodecStrategy`：
/// 能力决定「谁参赛、怎么排」，不把解不了的流硬喂给解码器）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderCapability {
    /// 内置 ByteVC2 解码器可用。
    pub bytevc2: bool,
    /// 只让内置软解器能解的编码参赛（HEVC 系与 ByteVC2）。开着时 H.264、没标编码、
    /// 认不出的编码一律不选——播放不靠系统解码器，也不转码，选了就是播不出来。
    pub builtin_only: bool,
}

impl DecoderCapability {
    /// 全能力、无 bytevc2（单测与下载路径默认）。
    pub const fn full() -> Self {
        Self {
            bytevc2: false,
            builtin_only: false,
        }
    }

    /// 内置软解直播放：HEVC 系与 ByteVC2 都由进程内解码器解，与系统装没装 HEVC 无关。
    pub const fn builtin() -> Self {
        Self {
            bytevc2: true,
            builtin_only: true,
        }
    }

    pub fn with_bytevc2(mut self, ok: bool) -> Self {
        self.bytevc2 = ok;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamScore {
    /// 清晰度等级
    pub def_num: u32,
    /// 像素数
    pub pixels: u32,
    /// 编码器偏好（越小越优先：HEVC > bytevc1 > bytevc2 > 其它）
    pub codec_rank: u8,
    /// 码率
    pub bitrate: u32,
}

/// 清晰度 > 像素 > 编码偏好 > 码率。
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
    pub file_hash: Option<String>,
    pub spade: String,
    pub codec: Option<String>,
    /// 实际选中的档位。发生回退时它是自动挑的那档，不是请求的那档。
    pub definition: u32,
}
