//! 平台硬编的一次转码请求（与 ffmpeg 侧的 `TranscodeRequest` 同构）。

use std::path::Path;

/// 平台硬编的一次转码请求。
pub struct PlatformRequest<'a> {
    /// 已解密的源 MP4
    pub input: &'a Path,
    /// 输出路径（内部先写临时文件，成功后原子替换）
    pub output: &'a Path,
    /// 统一到这个分辨率。`None` 表示保持源分辨率
    pub scale_to: Option<(u32, u32)>,
    /// 已编码秒数的回调（0.0–时长秒），用于集内进度
    pub on_progress: Option<&'a (dyn Fn(f64) + Send + Sync)>,
}
