//! 择优域数据模型（流的比较指标与挑中的流）。

use std::cmp::Ordering;

/// 本机解码能力对选流的约束（语义对齐官方 TTVideoEngine `CodecStrategy`：
/// 能力决定「谁参赛、怎么排」，不把解不了的流硬喂给解码器）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderCapability {
    /// 系统 HEVC 可解（WebView / 系统播放器可直喂 `h265`/`bytevc1`）。
    pub hevc: bool,
    /// 内置 Rust ByteVC2 解码器可用：`bytevc2` 可经转码播放。
    pub bytevc2: bool,
    /// 只让内置软解器能解的编码参赛（HEVC 系与 ByteVC2）。开着时 H.264、没标编码、
    /// 认不出的编码一律不选——播放不靠系统解码器，也不转码，选了就是播不出来。
    pub builtin_only: bool,
}

impl DecoderCapability {
    /// 全能力、无 bytevc2（单测与下载路径默认）。
    pub const fn full() -> Self {
        Self {
            hevc: true,
            bytevc2: false,
            builtin_only: false,
        }
    }

    /// 无 HEVC、无 bytevc2。
    pub const fn no_hevc() -> Self {
        Self {
            hevc: false,
            bytevc2: false,
            builtin_only: false,
        }
    }

    /// 内置软解直播放：HEVC 系与 ByteVC2 都由进程内解码器解，与系统装没装 HEVC 无关。
    pub const fn builtin() -> Self {
        Self {
            hevc: true,
            bytevc2: true,
            builtin_only: true,
        }
    }

    /// 仅按 HEVC 探测构造（bytevc2 默认关；生产请用 [`Self::from_probes`]）。
    pub fn from_hevc(playable: bool) -> Self {
        Self {
            hevc: playable,
            bytevc2: false,
            builtin_only: false,
        }
    }

    /// 按本机 HEVC + ByteVC2 探测结论选模式。
    pub fn from_probes(hevc: bool, bytevc2: bool) -> Self {
        Self {
            hevc,
            bytevc2,
            builtin_only: false,
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
    /// 编码器偏好（**越小越优先**：h265/h264 系 > bytevc1 > bytevc2 > 其它）
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
    pub file_hash: Option<String>,
    pub spade: String,
    pub codec: Option<String>,
    /// 实际选中的档位。发生回退时它是自动挑的那档，不是请求的那档。
    pub definition: u32,
}
