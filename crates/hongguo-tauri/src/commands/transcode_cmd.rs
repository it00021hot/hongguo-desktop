//! 转码能力查询。真正的转码实现在 `hongguo_media::mux` 与平台编码 crate，只由「兼容格式合并」
//! （`merge_service::compat`）驱动——播放不转码。缓存清理已并入 storage_cmd（`get_cache_usage` / `clear_cache`）。

use hongguo_media_platform::capability::probe::DecodeCapability;

/// 硬解/软解能力。
#[tauri::command]
pub fn decode_capability() -> DecodeCapability {
    hongguo_media_platform::capability::probe::detect()
}

/// 丢弃缓存重新探测一次 ffmpeg 与编码器。**用户装完 ffmpeg 之后进程的环境变量不会更新**，不重探就一直报「未检测到」——设置页点一下即可生效，不必重启应用（探测本身一到两秒）。
#[tauri::command]
pub fn redetect_capability() -> DecodeCapability {
    hongguo_media_platform::capability::probe::redetect()
}
