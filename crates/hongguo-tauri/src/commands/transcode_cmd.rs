//! 转码能力查询与播放兼容兜底。真正的转码实现在 `hongguo_media::mux` 与平台硬编 crate，由合并功能（`merge_service::compat`）与播放兼容兜底（`play_service::compat_play::compat_for_playback`）驱动，这里只留能力查询与转调；缓存清理已并入 storage_cmd（`get_cache_usage` / `clear_cache`）。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_media_platform::capability::probe::DecodeCapability;
use hongguo_service::app_state::AppState;
use hongguo_service::play_service::compat_play::CompatPlayResult;

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

/// 把一集转成 H.264。首段可播就返回地址，整片在同一路径上继续写。
#[tauri::command]
pub async fn transcode_for_playback(
    state: State<'_, AppState>,
    series_id: String,
    vid_index: u32,
    vid: Option<String>,
) -> AppResult<CompatPlayResult> {
    hongguo_service::play_service::compat_play::compat_for_playback_prefix(
        state.inner(),
        &series_id,
        vid_index,
        vid,
    )
    .await
}

/// 停止这一集还在写的兼容转码。
#[tauri::command]
pub fn cancel_compat_playback(series_id: String, vid_index: u32) {
    hongguo_service::play_service::compat_play::cancel_compat(&series_id, vid_index);
}
/// 离线 QA：本地明文样本走兼容转码首段（MSE）。正式模式返回 Unsupported。
#[tauri::command]
pub async fn qa_offline_local_play(
    state: State<'_, AppState>,
    file_path: String,
) -> AppResult<CompatPlayResult> {
    hongguo_service::play_service::compat_play::qa_offline_local_play(state.inner(), file_path).await
}
