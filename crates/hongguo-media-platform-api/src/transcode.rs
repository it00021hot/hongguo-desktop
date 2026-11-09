//! 平台硬编的一次转码请求（与 ffmpeg 侧的 `TranscodeRequest` 同构）。

use std::path::Path;

use crate::video::RateProfile;

/// 平台硬编的一次转码请求。
pub struct PlatformRequest<'a> {
    /// 已解密的源 MP4
    pub input: &'a Path,
    /// 输出路径（内部先写临时文件，成功后原子替换）
    pub output: &'a Path,
    /// 统一到这个分辨率。`None` 表示保持源分辨率
    pub scale_to: Option<(u32, u32)>,
    /// 码率档：合并走归档，兼容播放走播放档。
    pub rate: RateProfile,
    /// 已编码秒数的回调（0.0–时长秒），用于集内进度
    pub on_progress: Option<&'a (dyn Fn(f64) + Send + Sync)>,
    /// 首个可独立解码的 GOP 已写入 `output` 时调用。同一路径之后继续追加，总长此时还在涨。
    pub on_playable_prefix: Option<&'a (dyn Fn(&Path) + Send + Sync)>,
    /// 每写完一段可读取字节就回调当前文件长度。
    pub on_committed: Option<&'a (dyn Fn(u64) + Send + Sync)>,
    /// 为真时停止继续编码。
    pub is_cancelled: Option<&'a (dyn Fn() -> bool + Send + Sync)>,
}
