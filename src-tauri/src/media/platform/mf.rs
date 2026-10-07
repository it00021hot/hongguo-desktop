//! Windows Media Foundation 硬编后端（阶段 2）。
//!
//! 硬件 MFT 枚举（`MFT_ENUM_FLAG_HARDWARE` + 试编验证）→ async 事件循环。
//! 本文件先立住模块边界，Linux CI 不会编译到这里。

use crate::error::AppResult;

use super::PlatformRequest;

/// 平台硬编可用时执行转码；不可用返回 `None`。
pub fn transcode_h264(_req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    None
}

/// 平台硬编 H.264 编码器是否可用。
pub fn h264_hw_encoder_available() -> bool {
    false
}

/// 丢弃探测缓存。
pub fn clear_probe_cache() {}
