//! 转码能力查询。真正的转码实现在 `hongguo_media::mux` 与平台编码 crate，只由「兼容格式合并」
//! （`merge_service::compat`）驱动——播放不转码。缓存清理已并入 storage_cmd（`get_cache_usage` / `clear_cache`）。

use hongguo_media_platform::capability::probe::DecodeCapability;

/// 硬解/软解能力。
#[tauri::command]
pub fn decode_capability() -> DecodeCapability {
    hongguo_media_platform::capability::probe::detect()
}

/// 清除缓存并重新探测原生归档编码能力。
#[tauri::command]
pub fn redetect_capability() -> DecodeCapability {
    hongguo_media_platform::capability::probe::redetect()
}
