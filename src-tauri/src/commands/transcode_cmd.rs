//! 转码能力查询与转码缓存管理。
//!
//! 这里只暴露「能力查询」和「缓存清理」——真正的转码实现在
//! [`crate::media::transcode`]，由合并功能（`merge_service::compat`）驱动。
//!
//! 播放兜底已下线：WebView2 能否解 HEVC 由系统决定，为一台解不了的机器
//! 挂一条「黑屏了才现转、现转十几分钟」的链路不值得。

use crate::media::capability::probe::DecodeCapability;
use crate::service::transcode_service::cache;

/// 硬解/软解能力。
#[tauri::command]
pub fn decode_capability() -> DecodeCapability {
    crate::media::capability::probe::detect()
}

/// 转码缓存状态：目录、文件数、总字节。
#[tauri::command]
pub fn compat_cache_status() -> (String, usize, u64) {
    cache::status()
}

/// 清空转码缓存。
#[tauri::command]
pub fn clear_compat_cache() -> usize {
    cache::clear()
}

/// 在线流缓存状态。
#[tauri::command]
pub fn online_cache_status() -> (usize, u64) {
    crate::service::play_service::online::status()
}

/// 清空在线流缓存。
#[tauri::command]
pub fn clear_online_cache() -> usize {
    crate::service::play_service::online::clear()
}
